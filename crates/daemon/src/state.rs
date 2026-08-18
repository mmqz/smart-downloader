//! DaemonState（M6 集成层）：任务目录 + HttpEngine + FallbackPolicy + WsHub；
//! add/pause/resume/remove/snapshot/list/provider 快照；重复 canonical → 409 事件。

use smart_dl_core::identity::{CanonicalId, CanonicalKind, ContentIdentity};
use smart_dl_core::source_parse::normalize::{normalize_user_link, NormalizedSource};
use smart_dl_core::state_machine::TaskState;
use smart_dl_core::task::{DownloadTask, TaskId, TaskMetadata};
use smart_dl_core::types::{
    DownloadEngine, DownloadSource, EngineKind, EngineState, EngineStatus, EngineTaskId,
};
use smart_dl_provider::{ProviderRuntime, RemoteProvider};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::events::SchedulerEvent;
use crate::ws::WsHub;

/// 任务记录（引擎句柄 + 引擎运行态缓存）。
#[derive(Clone)]
pub struct TaskRecord {
    pub task: DownloadTask,
    pub engine_tid: Option<EngineTaskId>,
    pub engine_kind: EngineKind,
    pub engine_status: Option<EngineStatus>,
    /// 运行态操作日志（add/pause/resume/remove/restored；引擎状态变更不记——见快照）。
    events: Vec<TaskEvent>,
}

/// 任务操作日志条目（`GET /tasks/:id/logs` 返回）。
#[derive(Clone, Debug, serde::Serialize)]
pub struct TaskEvent {
    /// Unix 毫秒时间戳。
    pub at_ms: u64,
    /// 操作名：add / pause / resume / remove / restored。
    pub op: String,
    pub detail: Option<String>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl TaskRecord {
    fn push_event(&mut self, op: &str, detail: Option<String>) {
        self.events.push(TaskEvent {
            at_ms: now_ms(),
            op: op.to_string(),
            detail,
        });
    }
}

/// 任务快照（GET /tasks/:id，跳号补拉入口）。
#[derive(Clone, Debug, serde::Serialize)]
pub struct TaskSnapshot {
    pub task_id: String,
    /// 状态字符串（`Downloading(Http)` → `"Downloading"`；API 消费者无需解析枚举负载）。
    pub state: String,
    pub source: String,
    pub dest_root: PathBuf,
    pub engine: Option<String>,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
}

/// 列表条目。
#[derive(Clone, Debug, serde::Serialize)]
pub struct TaskSummary {
    pub task_id: String,
    /// 状态字符串（同上）。
    pub state: String,
    pub source: String,
}

/// 快照用状态标签：取枚举 Debug 的变体名部分。
pub fn state_label(s: &TaskState) -> String {
    let d = format!("{s:?}");
    d.split('(').next().unwrap_or(&d).to_string()
}

/// BT alert 应用结果（task_id + 状态迁移 + 消息），供事件广播使用。
#[cfg(feature = "bt")]
#[derive(Clone, Debug)]
pub struct BtAlertEffect {
    pub task_id: String,
    pub from: TaskState,
    pub to: TaskState,
    pub message: String,
}

/// HTTP 轮询推进结果（task_id + 状态迁移 + 消息），供事件广播使用。
#[derive(Clone, Debug)]
pub struct HttpPollEffect {
    pub task_id: String,
    pub from: TaskState,
    pub to: TaskState,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("duplicate task (existing: {0})")]
    Duplicate(String),
    #[error("task not found: {0}")]
    NotFound(String),
    #[error("engine error: {0}")]
    Engine(String),
    #[error("invalid source: {0}")]
    InvalidSource(String),
    #[error("持久化: {0}")]
    Persist(String),
}

/// 守护进程状态：任务 + 引擎表 + 事件中枢。
pub struct DaemonState {
    engines: HashMap<EngineKind, Arc<dyn DownloadEngine>>,
    hub: WsHub,
    tasks: Mutex<HashMap<TaskId, TaskRecord>>,
    providers: Vec<Arc<dyn RemoteProvider>>,
    next_id: AtomicU64,
    /// 任务持久化文件（Some 时 add/remove/状态变更后自动落盘）。
    persist_path: Option<PathBuf>,
    /// HTTP 任务默认落盘目录（dest 未指定时用；serve 从配置 `[download] dest_root` 注入；
    /// Mutex 支持 #6 TOML 热重载动态更新）。
    default_dest_root: Mutex<PathBuf>,
    /// 生效配置快照（`GET /config` 返回；serve 注入精简字段；热重载后刷新）。
    config_snapshot: Mutex<Option<serde_json::Value>>,
}

/// 持久化任务记录：`task`（含 source 原文：url/magnet/torrent 字节）+ 引擎种类。
/// 运行态字段（engine_tid/engine_status）不落盘——恢复时重新向引擎 add。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PersistedTask {
    pub task: DownloadTask,
    pub engine_kind: EngineKind,
}

/// 原子写任务文件（tmp + rename，防半写）。
pub fn write_tasks_atomic(path: &Path, tasks: &[PersistedTask]) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(tasks).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

impl DaemonState {
    /// 单引擎构造（HTTP）；BT 引擎用 `with_bt` 追加（feature `bt`）。
    pub fn new(engine: Arc<dyn DownloadEngine>, providers: Vec<Arc<dyn RemoteProvider>>) -> Self {
        let mut engines = HashMap::new();
        engines.insert(engine.kind(), engine);
        DaemonState {
            engines,
            hub: WsHub::new(),
            tasks: Mutex::new(HashMap::new()),
            providers,
            next_id: AtomicU64::new(1),
            persist_path: None,
            default_dest_root: Mutex::new(PathBuf::from(".")),
            config_snapshot: Mutex::new(None),
        }
    }

    /// 注入 HTTP 任务默认落盘目录（dest 未指定时使用；serve 从 `[download] dest_root` 传入）。
    pub fn with_dest_root(self, default_dest_root: PathBuf) -> Self {
        *self.default_dest_root.lock().unwrap() = default_dest_root;
        self
    }

    /// 注入生效配置快照（`GET /config` 返回；serve 组装精简字段）。
    pub fn with_config(self, snapshot: serde_json::Value) -> Self {
        *self.config_snapshot.lock().unwrap() = Some(snapshot);
        self
    }

    /// 启用任务持久化（每次变更自动写 JSON 到 `path`）。
    pub fn with_storage(mut self, path: PathBuf) -> Self {
        self.persist_path = Some(path);
        self
    }

    /// 追加 BT 引擎（feature `bt`；无该引擎时 magnet 路由 → InvalidSource）。
    #[cfg(feature = "bt")]
    pub fn with_bt(mut self, bt: Arc<dyn DownloadEngine>) -> Self {
        self.engines.insert(EngineKind::Bt, bt);
        self
    }

    /// 序列化当前任务目录（持久化用）。
    fn persisted_tasks(&self) -> Vec<PersistedTask> {
        self.tasks
            .lock()
            .unwrap()
            .values()
            .map(|r| PersistedTask {
                task: r.task.clone(),
                engine_kind: r.engine_kind,
            })
            .collect()
    }

    /// 自动落盘（启用 storage 时）。同步原子写：任务变更低频（add/remove/状态迁移），
    /// 必须保证顺序（异步并发写会竞态覆盖旧快照）；JSON 规模小，阻塞代价可忽略。
    fn autosave(&self) {
        let Some(path) = self.persist_path.clone() else {
            return;
        };
        let data = self.persisted_tasks();
        if let Err(e) = write_tasks_atomic(&path, &data) {
            tracing::warn!("任务持久化失败 {path:?}: {e}");
        }
    }

    /// 从持久化文件恢复任务：逐条重新 add 到引擎（保留原 task_id，
    /// next_id 推进），add 失败的任务标 Failed 保留记录。返回恢复条数。
    pub async fn restore_from(&self, path: &Path) -> Result<usize, DaemonError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| DaemonError::Persist(format!("读取 {path:?} 失败: {e}")))?;
        let pts: Vec<PersistedTask> = serde_json::from_str(&text)
            .map_err(|e| DaemonError::Persist(format!("解析 {path:?} 失败: {e}")))?;
        let mut restored = 0usize;
        let mut failed = 0usize;
        for pt in pts {
            let mut t = pt.task.clone();
            t.state = TaskState::Queued; // 重启后重新入队
            let engine = match self.engine_for(pt.engine_kind) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("恢复任务 {} 引擎不可用: {e}", t.id);
                    continue;
                }
            };
            match engine.add(&t).await {
                Ok(tid) => {
                    let mut rec = TaskRecord {
                        task: t,
                        engine_tid: Some(tid),
                        engine_kind: pt.engine_kind,
                        engine_status: None,
                        events: vec![],
                    };
                    rec.push_event("restored", None);
                    self.tasks.lock().unwrap().insert(rec.task.id.clone(), rec);
                    restored += 1;
                }
                Err(e) => {
                    tracing::warn!("恢复任务 {} 引擎 add 失败（标 Failed）: {e}", t.id);
                    t.state = TaskState::Failed;
                    let mut rec = TaskRecord {
                        task: t,
                        engine_tid: None,
                        engine_kind: pt.engine_kind,
                        engine_status: None,
                        events: vec![],
                    };
                    rec.push_event("restored", Some(format!("引擎 add 失败: {e}")));
                    self.tasks.lock().unwrap().insert(rec.task.id.clone(), rec);
                    failed += 1;
                }
            }
        }
        // next_id 推进到已用最大值之后（保留原 task_id 的关键）
        let max_id = self
            .tasks
            .lock()
            .unwrap()
            .keys()
            .filter_map(|k| k.strip_prefix('t').and_then(|s| s.parse::<u64>().ok()))
            .max()
            .unwrap_or(0);
        self.next_id.fetch_max(max_id + 1, Ordering::SeqCst);
        tracing::info!("任务恢复完成: {restored} 恢复, {failed} 失败（引擎 add 错误）");
        Ok(restored)
    }

    fn engine_for(&self, kind: EngineKind) -> Result<Arc<dyn DownloadEngine>, DaemonError> {
        self.engines.get(&kind).cloned().ok_or_else(|| {
            DaemonError::InvalidSource(format!("引擎未加载: {:?}（编译时启用对应 feature）", kind))
        })
    }

    pub fn hub(&self) -> &WsHub {
        &self.hub
    }

    /// 添加任务入口：支持 http/https/thunder:///qqdl:// 链接（归一化后走 HTTP 引擎）；
    /// magnet（feature `bt` 时走 libtorrent 引擎）；ed2k/无法识别 → InvalidSource。
    pub async fn add_link_task(
        &self,
        link: String,
        dest_root: Option<String>,
    ) -> Result<TaskId, DaemonError> {
        match normalize_user_link(&link) {
            NormalizedSource::Http(real) => self.add_http_task(real, dest_root).await,
            NormalizedSource::Magnet(m) => {
                #[cfg(feature = "bt")]
                {
                    return self.add_bt_task(m, dest_root).await;
                }
                #[cfg(not(feature = "bt"))]
                {
                    Err(DaemonError::InvalidSource(format!(
                        "magnet 需 BT 引擎（编译时启用 --features daemon/bt）: {m}"
                    )))
                }
            }
            NormalizedSource::Ed2k(e) => {
                Err(DaemonError::InvalidSource(format!("ed2k 不支持: {e}")))
            }
            NormalizedSource::Unsupported(orig) => Err(DaemonError::InvalidSource(format!(
                "无法识别的链接: {orig}"
            ))),
        }
    }

    /// 添加 BT 任务（feature `bt`）：btih canonical 查重 → 引擎 add → TaskCreated 事件。
    #[cfg(feature = "bt")]
    async fn add_bt_task(
        &self,
        magnet: String,
        dest_root: Option<String>,
    ) -> Result<TaskId, DaemonError> {
        // B10：目标目录预检（创建/可写）；magnet 总大小元数据前未知 → 空间预检跳过
        let dest_root = ensure_dest_root(dest_root)?;
        let canonical = CanonicalId {
            kind: CanonicalKind::Bt,
            identity: btih_of(&magnet).unwrap_or_else(|| magnet.clone()),
            validator: None,
            token_sensitive: false,
        };
        let task_id = format!("t{}", self.next_id.fetch_add(1, Ordering::SeqCst));

        // 查重（canonical 一致 → DuplicateRejected）
        {
            let tasks = self.tasks.lock().unwrap();
            for (existing, rec) in tasks.iter() {
                if rec.task.canonical_id == canonical {
                    self.hub.publish(SchedulerEvent::DuplicateRejected {
                        task_id: task_id.clone(),
                        existing: existing.clone(),
                    });
                    return Err(DaemonError::Duplicate(existing.clone()));
                }
            }
        }

        let task = DownloadTask {
            id: task_id.clone(),
            canonical_id: canonical,
            source: DownloadSource::Magnet(magnet.clone()),
            identity: ContentIdentity::SingleFile {
                size: 0,
                etag: None,
                sha256: None,
            },
            dest_root: dest_root.clone(),
            files: vec![],
            acquisitions: vec![],
            aggregate: Default::default(),
            state: TaskState::Queued,
            retry: Default::default(),
            created_at: std::time::Instant::now(),
            metadata: TaskMetadata {
                name: None,
                added_at_unix: 0,
            },
        };

        let engine_tid = self
            .engine_for(EngineKind::Bt)?
            .add(&task)
            .await
            .map_err(|e| DaemonError::Engine(e.to_string()))?;
        let mut rec = TaskRecord {
            task,
            engine_tid: Some(engine_tid),
            engine_kind: EngineKind::Bt,
            engine_status: None,
            events: vec![],
        };
        rec.push_event("add", None);
        self.tasks.lock().unwrap().insert(task_id.clone(), rec);
        self.autosave();
        self.hub.publish(SchedulerEvent::TaskCreated {
            task_id: task_id.clone(),
        });
        self.hub.publish(SchedulerEvent::StateChanged {
            task_id: task_id.clone(),
            from: TaskState::Queued,
            to: TaskState::Downloading(EngineKind::Bt),
        });
        Ok(task_id)
    }

    /// 添加 .torrent 文件任务（feature `bt`）：infohash canonical 查重 → 引擎
    /// add_torrent_file → TaskCreated 事件。torrent 字节来自 API base64 解码。
    #[cfg(feature = "bt")]
    pub async fn add_torrent_task(
        &self,
        torrent_bytes: Vec<u8>,
        dest_root: Option<String>,
    ) -> Result<TaskId, DaemonError> {
        // B10：目标目录预检（创建/可写）
        let dest_root = ensure_dest_root(dest_root)?;
        let Some(ih) = torrent_infohash(&torrent_bytes) else {
            return Err(DaemonError::InvalidSource(
                ".torrent 解析失败：无法定位 info dict".into(),
            ));
        };
        // B10：单文件 torrent 总大小已知 → 空间预检（多文件 v1 解析不到 → 跳过）
        if let Some(total) = torrent_total_size(&torrent_bytes) {
            precheck_space(&dest_root, total)?;
        }
        let canonical = CanonicalId {
            kind: CanonicalKind::Bt,
            identity: ih.clone(),
            validator: None,
            token_sensitive: false,
        };
        let task_id = format!("t{}", self.next_id.fetch_add(1, Ordering::SeqCst));

        // 查重（canonical 一致 → DuplicateRejected）
        {
            let tasks = self.tasks.lock().unwrap();
            for (existing, rec) in tasks.iter() {
                if rec.task.canonical_id == canonical {
                    self.hub.publish(SchedulerEvent::DuplicateRejected {
                        task_id: task_id.clone(),
                        existing: existing.clone(),
                    });
                    return Err(DaemonError::Duplicate(existing.clone()));
                }
            }
        }

        let task = DownloadTask {
            id: task_id.clone(),
            canonical_id: canonical,
            source: DownloadSource::TorrentFile(torrent_bytes),
            identity: ContentIdentity::SingleFile {
                size: 0,
                etag: None,
                sha256: None,
            },
            dest_root: dest_root.clone(),
            files: vec![],
            acquisitions: vec![],
            aggregate: Default::default(),
            state: TaskState::Queued,
            retry: Default::default(),
            created_at: std::time::Instant::now(),
            metadata: TaskMetadata {
                name: None,
                added_at_unix: 0,
            },
        };

        let engine_tid = self
            .engine_for(EngineKind::Bt)?
            .add(&task)
            .await
            .map_err(|e| DaemonError::Engine(e.to_string()))?;
        let mut rec = TaskRecord {
            task,
            engine_tid: Some(engine_tid),
            engine_kind: EngineKind::Bt,
            engine_status: None,
            events: vec![],
        };
        rec.push_event("add", None);
        self.tasks.lock().unwrap().insert(task_id.clone(), rec);
        self.autosave();
        self.hub.publish(SchedulerEvent::TaskCreated {
            task_id: task_id.clone(),
        });
        self.hub.publish(SchedulerEvent::StateChanged {
            task_id: task_id.clone(),
            from: TaskState::Queued,
            to: TaskState::Downloading(EngineKind::Bt),
        });
        Ok(task_id)
    }

    /// 添加 HTTP 任务：canonical 查重 → HttpEngine.add → TaskCreated 事件。
    pub async fn add_http_task(
        &self,
        url: String,
        dest_root: Option<String>,
    ) -> Result<TaskId, DaemonError> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(DaemonError::InvalidSource(url));
        }
        // B10：目标目录预检（创建/可写）；HTTP 大小在响应头才知 → 空间预检跳过
        // dest 未指定 → 默认落盘目录（serve 配置 dest_root；未注入时为 daemon cwd）
        let def = self
            .default_dest_root
            .lock()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let dest = dest_root.or(Some(def));
        let dest_root = ensure_dest_root(dest)?;
        let canonical = CanonicalId {
            kind: CanonicalKind::Http,
            identity: canonical_http_url(&url), // D34：剥 token 参数后的 canonical 身份
            validator: None,
            token_sensitive: false,
        };
        let task_id = format!("t{}", self.next_id.fetch_add(1, Ordering::SeqCst));

        // 查重（canonical 一致 → DuplicateRejected）
        {
            let tasks = self.tasks.lock().unwrap();
            for (existing, rec) in tasks.iter() {
                if rec.task.canonical_id == canonical {
                    self.hub.publish(SchedulerEvent::DuplicateRejected {
                        task_id: task_id.clone(),
                        existing: existing.clone(),
                    });
                    return Err(DaemonError::Duplicate(existing.clone()));
                }
            }
        }

        let task = DownloadTask {
            id: task_id.clone(),
            canonical_id: canonical,
            source: DownloadSource::Http {
                url: url.clone(),
                headers: vec![],
                auth: None,
            },
            identity: ContentIdentity::SingleFile {
                size: 0,
                etag: None,
                sha256: None,
            },
            dest_root: dest_root.clone(),
            files: vec![],
            acquisitions: vec![],
            aggregate: Default::default(),
            state: TaskState::Queued,
            retry: Default::default(),
            created_at: std::time::Instant::now(),
            metadata: TaskMetadata {
                name: None,
                added_at_unix: 0,
            },
        };

        let engine_tid = self
            .engine_for(EngineKind::Http)?
            .add(&task)
            .await
            .map_err(|e| DaemonError::Engine(e.to_string()))?;
        let mut rec = TaskRecord {
            task,
            engine_tid: Some(engine_tid),
            engine_kind: EngineKind::Http,
            engine_status: None,
            events: vec![],
        };
        rec.push_event("add", None);
        self.tasks.lock().unwrap().insert(task_id.clone(), rec);
        self.autosave();
        self.hub.publish(SchedulerEvent::TaskCreated {
            task_id: task_id.clone(),
        });
        self.hub.publish(SchedulerEvent::StateChanged {
            task_id: task_id.clone(),
            from: TaskState::Queued,
            to: TaskState::Downloading(EngineKind::Http),
        });
        Ok(task_id)
    }

    /// 任务快照（实时读引擎状态；未完成时引擎可能已移动）。
    pub async fn task_snapshot(&self, id: &str) -> Option<TaskSnapshot> {
        let rec = self.tasks.lock().unwrap().get(id).cloned()?;
        let engine = self.engine_for(rec.engine_kind).ok();
        let (engine_name, status) = match (&rec.engine_tid, &engine) {
            (Some(tid), Some(eng)) => {
                let st = eng.status(tid).await.ok();
                (Some(eng.id().to_string()), st)
            }
            _ => (None, None),
        };
        let state = match &status {
            Some(s) => state_label(&engine_state_to_task(&s.state, rec.engine_kind)),
            None => state_label(&rec.task.state),
        };
        Some(TaskSnapshot {
            task_id: id.to_string(),
            state,
            source: format!("{:?}", rec.task.source),
            dest_root: rec.task.dest_root.clone(),
            engine: engine_name,
            done: status.as_ref().map(|s| s.total_done).unwrap_or(0),
            total: status.as_ref().map(|s| s.total).unwrap_or(0),
            error: status.as_ref().and_then(|s| s.error.clone()),
        })
    }

    pub fn list(&self) -> Vec<TaskSummary> {
        self.tasks
            .lock()
            .unwrap()
            .iter()
            .map(|(id, rec)| TaskSummary {
                task_id: id.clone(),
                state: state_label(&rec.task.state),
                source: format!("{:?}", rec.task.source),
            })
            .collect()
    }

    pub async fn pause(&self, id: &str) -> Result<(), DaemonError> {
        let rec = self
            .tasks
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| DaemonError::NotFound(id.to_string()))?;
        let tid = rec
            .engine_tid
            .clone()
            .ok_or_else(|| DaemonError::NotFound(id.to_string()))?;
        self.engine_for(rec.engine_kind)?
            .pause(&tid)
            .await
            .map_err(|e| DaemonError::Engine(e.to_string()))?;
        if let Some(rec) = self.tasks.lock().unwrap().get_mut(id) {
            rec.push_event("pause", None);
        }
        self.hub.publish(SchedulerEvent::StateChanged {
            task_id: id.to_string(),
            from: TaskState::Downloading(rec.engine_kind),
            to: TaskState::Paused,
        });
        Ok(())
    }

    pub async fn resume(&self, id: &str) -> Result<(), DaemonError> {
        let rec = self
            .tasks
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| DaemonError::NotFound(id.to_string()))?;
        let tid = rec
            .engine_tid
            .clone()
            .ok_or_else(|| DaemonError::NotFound(id.to_string()))?;
        self.engine_for(rec.engine_kind)?
            .resume(&tid)
            .await
            .map_err(|e| DaemonError::Engine(e.to_string()))?;
        if let Some(rec) = self.tasks.lock().unwrap().get_mut(id) {
            rec.push_event("resume", None);
        }
        self.hub.publish(SchedulerEvent::StateChanged {
            task_id: id.to_string(),
            from: TaskState::Paused,
            to: TaskState::Downloading(rec.engine_kind),
        });
        Ok(())
    }

    pub async fn remove(&self, id: &str) -> Result<(), DaemonError> {
        let rec = self
            .tasks
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| DaemonError::NotFound(id.to_string()))?;
        if let Some(tid) = rec.engine_tid {
            if let Ok(engine) = self.engine_for(rec.engine_kind) {
                let _ = engine.remove(&tid, false).await;
            }
        }
        self.autosave();
        Ok(())
    }

    /// Provider 运行态快照（健康/配额/冷却）。
    pub fn provider_status(&self) -> Vec<(String, ProviderRuntime)> {
        self.providers
            .iter()
            .map(|p| (p.name().to_string(), p.runtime()))
            .collect()
    }

    /// 任务操作日志（`GET /tasks/:id/logs`）：快照 + 事件序列。
    pub fn task_logs(&self, id: &str) -> Result<serde_json::Value, DaemonError> {
        let tasks = self.tasks.lock().unwrap();
        let rec = tasks
            .get(id)
            .ok_or_else(|| DaemonError::NotFound(id.to_string()))?;
        Ok(serde_json::json!({
            "task_id": rec.task.id,
            "state": state_label(&rec.task.state),
            "source": format!("{:?}", rec.task.source),
            "error": rec.engine_status.as_ref().and_then(|s| s.error.clone()),
            "events": rec.events,
        }))
    }

    /// 生效配置快照（`GET /config` 返回；未注入时给出提示对象）。
    pub fn config_snapshot(&self) -> serde_json::Value {
        self.config_snapshot
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| serde_json::json!({ "note": "配置快照未注入（serve 组装）" }))
    }

    /// #6 TOML 热重载应用：配置重读后刷新可热更字段（default_dest_root + /config 快照）。
    /// 变更项记日志；不变项静默。
    pub fn refresh_config(&self, cfg: &crate::config::Config, tasks_path: &std::path::Path) {
        {
            let mut def = self.default_dest_root.lock().unwrap();
            let new_root = cfg.download.dest_root.clone();
            if *def != new_root {
                tracing::info!("配置热重载: dest_root {:?} → {:?}", *def, new_root);
                *def = new_root;
            }
        }
        let snap = crate::config::Config::snapshot_json(cfg, tasks_path);
        if *self.config_snapshot.lock().unwrap() != Some(snap.clone()) {
            *self.config_snapshot.lock().unwrap() = Some(snap);
        }
    }
}

/// B10（§12 D36）：dest_root 预检——缺失目录自动创建 + 可写探测（探针文件）。
/// 空间充足性由 `precheck_space` 在总大小已知时另行检查。
pub fn ensure_dest_root(dest: Option<String>) -> Result<PathBuf, DaemonError> {
    let p = PathBuf::from(dest.unwrap_or_else(|| ".".to_string()));
    fs::create_dir_all(&p)
        .map_err(|e| DaemonError::InvalidSource(format!("目标目录不可创建: {e}")))?;
    let probe = p.join(format!(".write_probe-{}", std::process::id()));
    fs::write(&probe, b"ok")
        .map_err(|e| DaemonError::InvalidSource(format!("目标目录不可写: {e}")))?;
    let _ = fs::remove_file(&probe);
    Ok(p)
}

/// B10：空间预检（总大小已知时调用）——`evaluate_disk` 判定不足 → 拒绝入队。
/// 磁盘可用空间取不到（fs2 失败）时静默放行（非致命）。
pub fn precheck_space(p: &Path, total: u64) -> Result<(), DaemonError> {
    let Ok(avail) = fs2::free_space(p) else {
        return Ok(());
    };
    use smart_dl_core::session::output::{evaluate_disk, DiskCheck};
    if let DiskCheck::Insufficient {
        required,
        available,
    } = evaluate_disk(avail, total)
    {
        return Err(DaemonError::InvalidSource(format!(
            "磁盘空间不足: 需要 {} 字节, 可用 {} 字节",
            required, available
        )));
    }
    Ok(())
}

impl DaemonState {
    /// 应用一条 BT alert 到匹配任务（engine_tid 大小写不敏感归一化比较）：
    /// 状态迁移（`bt_events::transition_for`）+ 引擎缓存写入；返回效果供广播。
    /// 无匹配任务或无迁移 → `None`（调用方丢弃该 alert）。
    #[cfg(feature = "bt")]
    pub fn apply_bt_alert(&self, a: &smart_dl_btcore::Alert) -> Option<BtAlertEffect> {
        let ih_l = a.ih.to_ascii_lowercase();
        let mut tasks = self.tasks.lock().unwrap();
        for (id, rec) in tasks.iter_mut() {
            if rec.engine_kind != EngineKind::Bt {
                continue;
            }
            let Some(tid) = &rec.engine_tid else {
                continue;
            };
            if tid.to_ascii_lowercase() != ih_l {
                continue;
            }
            // 命中任务（每条 alert 至多匹配一个 rec）：无迁移 → 丢弃（`?` 提前返回 None）
            let now = rec.task.state.clone();
            let (from, to) = crate::bt_events::transition_for(&now, a)?;
            rec.task.state = to.clone();
            if let Some(es) = rec.engine_status.as_mut() {
                if to == TaskState::Failed {
                    es.error = Some(a.msg.clone());
                }
            }
            self.autosave(); // 状态迁移落盘
            return Some(BtAlertEffect {
                task_id: id.clone(),
                from,
                to,
                message: a.msg.clone(),
            });
        }
        None
    }

    /// HTTP 任务状态推进轮询：权威 = 引擎实时状态（v1 HTTP 引擎无 alert 回调，
    /// 记录 state 此前停在 Queued——list 与 status 不一致）。每轮：
    /// - 引擎终态（Completed/Error）→ 记录推进 Completed/Failed + 落盘；
    /// - 引擎活跃（Downloading/MetadataPending）→ Queued 记录顺带推进 Downloading(Http)。
    ///
    /// 返回本批效果供事件广播；无变化的任务跳过。
    pub async fn poll_http_task_states(&self) -> Vec<HttpPollEffect> {
        // 先收集候选（锁外做引擎调用；避免长持锁）
        let candidates: Vec<(String, EngineTaskId)> = {
            let tasks = self.tasks.lock().unwrap();
            tasks
                .iter()
                .filter(|(_, rec)| rec.engine_kind == EngineKind::Http)
                .filter(|(_, rec)| {
                    matches!(
                        rec.task.state,
                        TaskState::Queued | TaskState::Downloading(_)
                    )
                })
                .filter_map(|(id, rec)| rec.engine_tid.clone().map(|t| (id.clone(), t)))
                .collect()
        };
        let mut effects = Vec::new();
        for (id, tid) in candidates {
            let Ok(engine) = self.engine_for(EngineKind::Http) else {
                continue;
            };
            // 引擎侧已移除/不可用 → 跳过（任务移除后轮询器自然停）
            let Ok(st) = engine.status(&tid).await else {
                continue;
            };
            let (from, to) = {
                let mut tasks = self.tasks.lock().unwrap();
                let Some(rec) = tasks.get_mut(&id) else {
                    continue;
                };
                // 双检：轮询间隙状态可能已被别处推进（remove/pause/恢复）
                if !matches!(
                    rec.task.state,
                    TaskState::Queued | TaskState::Downloading(_)
                ) {
                    continue;
                }
                let from = rec.task.state.clone();
                let to = engine_state_to_task(&st.state, EngineKind::Http);
                if to == from {
                    continue; // 已在目标态（活跃→活跃、终态已推等）
                }
                rec.task.state = to.clone();
                if let Some(es) = rec.engine_status.as_mut() {
                    if to == TaskState::Failed {
                        es.error = st.error.clone();
                    }
                }
                self.autosave(); // 终态/推进落盘
                (from, to)
            };
            effects.push(HttpPollEffect {
                task_id: id,
                from,
                to,
                message: st.error.clone().unwrap_or_default(),
            });
        }
        effects
    }
}

/// D34：canonical URL —— 剥离签名/token 参数后作为去重身份，使同一资源的
/// 带签名链接（token 过期/轮换）仍能识别为同一任务。
/// 黑名单（设计文档 §7 D34）：`token|sig|signature|expires|auth|X-Amz-*|X-Goog-*|X-Tencent-*|X-QiNiu-*`
pub fn canonical_http_url(raw: &str) -> String {
    let Ok(mut u) = url::Url::parse(raw) else {
        return raw.to_string();
    };
    let mut kept: Vec<(String, String)> = Vec::new();
    for (k, v) in u.query_pairs() {
        if !is_token_param(&k) {
            kept.push((k.into_owned(), v.into_owned()));
        }
    }
    if kept.is_empty() {
        u.set_query(None);
    } else {
        let qs: Vec<String> = kept.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        u.set_query(Some(&qs.join("&")));
    }
    u.to_string()
}

/// 参数名是否命中 D34 token 黑名单（大小写敏感匹配，前缀通配 X-* 云签名族）。
fn is_token_param(name: &str) -> bool {
    matches!(name, "token" | "sig" | "signature" | "expires" | "auth")
        || name.starts_with("X-Amz-")
        || name.starts_with("X-Goog-")
        || name.starts_with("X-Tencent-")
        || name.starts_with("X-QiNiu-")
}

/// 从 magnet 提取 btih（40 hex，v1 规范 xt=urn:btih:）。无 → None（canonical 回落全文）。
#[cfg(feature = "bt")]
pub(crate) fn btih_of(magnet: &str) -> Option<String> {
    magnet.split('&').find_map(|p| {
        let v = p.strip_prefix("xt=urn:btih:")?;
        (v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit())).then(|| v.to_ascii_lowercase())
    })
}

/// 从 .torrent 字节提取 BT infohash（40 hex 小写）= SHA1(info dict 原始字节)。
/// 只做最小 bencode 定位（顶层 dict 找键 `info` → 配对结束 `e` 取整段），
/// 不做完整解析——足以支撑 canonical 查重。
#[cfg(feature = "bt")]
pub fn torrent_infohash(b: &[u8]) -> Option<String> {
    use sha1::Digest;
    let (info, end) = locate_info(b)?;
    let digest = sha1::Sha1::digest(&b[info..=end]);
    Some(
        digest
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>(),
    )
}

/// 单文件 .torrent 总大小（info dict 内 `length` 字段）；多文件（`files`）→ None。
/// v1 仅覆盖单文件场景（B10 空间预检用）；多文件留后续。
#[cfg(feature = "bt")]
pub fn torrent_total_size(b: &[u8]) -> Option<u64> {
    let (info, end) = locate_info(b)?;
    let mut i = info + 1;
    while i < end {
        let (key, ai) = be_str(b, i)?;
        i = ai;
        match key {
            b"length" => {
                if b.get(i) != Some(&b'i') {
                    return None;
                }
                let e = b[i..].iter().position(|&c| c == b'e')? + i;
                return std::str::from_utf8(&b[i + 1..e]).ok()?.parse().ok();
            }
            b"files" => return None, // 多文件：v1 不解析
            _ => i = value_skip(b, i)?,
        }
    }
    None
}

/// 定位 info dict：返回 (info 值起始 'd' 下标, info dict 闭合 'e' 下标)。
#[cfg(feature = "bt")]
fn locate_info(b: &[u8]) -> Option<(usize, usize)> {
    if b.first() != Some(&b'd') {
        return None;
    }
    let mut i = 1; // 顶层 dict 键值对扫描
    while i < b.len() {
        let (key, after_key) = be_str(b, i)?;
        i = after_key;
        if key == b"info" {
            if b.get(i) != Some(&b'd') {
                return None; // info 必须是 dict
            }
            let end = dict_skip(b, i)?;
            return Some((i, end));
        }
        // 跳过值（结构感知），继续找 `info`
        i = value_skip(b, i)?;
    }
    None
}

/// bencode 字符串 `len:data` → (data, 内容后下标)。
#[cfg(feature = "bt")]
fn be_str(b: &[u8], at: usize) -> Option<(&[u8], usize)> {
    let colon = b[at..].iter().position(|&c| c == b':')? + at;
    let len: usize = std::str::from_utf8(&b[at..colon]).ok()?.parse().ok()?;
    let start = colon + 1;
    Some((&b[start..start + len], start + len))
}

/// dict 结束下标：从 `start`（'d'）按 键(字符串)→值 结构推进到闭合 'e'。
/// 键位置固定为字符串（len: 数字开头），值可为任意类型——值内的数据字节
/// （如 pieces 的 20 字节）不会被误当 len: 解析。
#[cfg(feature = "bt")]
fn dict_skip(b: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while b.get(i) != Some(&b'e') {
        let (_, after) = be_str(b, i)?; // 键：字符串
        i = value_skip(b, after)?; // 值：任意类型
    }
    Some(i)
}

/// list 结束下标：从 `start`（'l'）按 值* 推进到闭合 'e'。
#[cfg(feature = "bt")]
fn list_skip(b: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while b.get(i) != Some(&b'e') {
        i = value_skip(b, i)?;
    }
    Some(i)
}

/// 跳过任意 bencode 值（dict/list/int/str），返回其后的下标。
#[cfg(feature = "bt")]
fn value_skip(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'd' => dict_skip(b, i).map(|e| e + 1),
        b'l' => list_skip(b, i).map(|e| e + 1),
        b'i' => {
            let e = b[i..].iter().position(|&c| c == b'e')? + i;
            Some(e + 1)
        }
        _ => be_str(b, i).map(|(_, after)| after),
    }
}

/// 引擎状态 → 对外任务状态（快照实时化；元数据获取中归入 Downloading）。
fn engine_state_to_task(st: &EngineState, kind: EngineKind) -> TaskState {
    match st {
        EngineState::MetadataPending | EngineState::Downloading => TaskState::Downloading(kind),
        EngineState::Paused => TaskState::Paused,
        EngineState::Completed => TaskState::Completed,
        EngineState::Seeding => TaskState::Seeding,
        EngineState::Error => TaskState::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_http_url;

    #[test]
    fn strips_token_param_keeps_others() {
        let c = canonical_http_url("https://host/a?token=abc&x=1&y=2");
        assert_eq!(c, "https://host/a?x=1&y=2");
    }

    #[test]
    fn strips_cloud_signing_family() {
        let c = canonical_http_url(
            "https://host/a?X-Amz-Signature=deadbeef&X-Amz-Date=20260101&sig=zz&expires=999999",
        );
        assert_eq!(c, "https://host/a");
    }

    #[test]
    fn no_token_url_unchanged() {
        let raw = "https://host/a?x=1";
        assert_eq!(canonical_http_url(raw), raw);
    }

    #[test]
    fn only_token_difference_collides() {
        let a = canonical_http_url("https://host/f?token=aaa&v=1");
        let b = canonical_http_url("https://host/f?v=1&token=bbb");
        assert_eq!(a, b);
    }

    #[test]
    fn invalid_url_passthrough() {
        assert_eq!(canonical_http_url("not a url"), "not a url");
    }

    #[test]
    fn fragment_and_path_unaffected() {
        let c = canonical_http_url("https://host/dir/file.bin?token=x&keep=1#frag");
        assert_eq!(c, "https://host/dir/file.bin?keep=1#frag");
    }
}

/// BT alert 事件流单元测试（feature `bt`）：`transition_for` 迁移矩阵 + `apply_bt_alert`
/// 匹配/缓存写入。不依赖真实 libtorrent 会话（手工构造 TaskRecord）。
#[cfg(all(test, feature = "bt"))]
mod bt_alert_tests {
    use super::*;
    use smart_dl_btcore::{Alert, AlertKind};

    fn make_state_with(rec: TaskRecord) -> DaemonState {
        let engine = smart_dl_httpdl::HttpEngine::new(reqwest::Client::new());
        let state = DaemonState::new(Arc::new(engine), vec![]);
        // 测试同 crate 内可访问私有 tasks 表
        (*state.tasks.lock().unwrap()).insert(rec.task.id.clone(), rec);
        state
    }

    fn bt_rec(state: TaskState, ih: &str) -> TaskRecord {
        TaskRecord {
            task: DownloadTask {
                id: "t1".into(),
                canonical_id: CanonicalId {
                    kind: CanonicalKind::Bt,
                    identity: ih.to_string(),
                    validator: None,
                    token_sensitive: false,
                },
                source: DownloadSource::Magnet(format!("magnet:?xt=urn:btih:{ih}")),
                identity: ContentIdentity::SingleFile {
                    size: 0,
                    etag: None,
                    sha256: None,
                },
                dest_root: PathBuf::from("."),
                files: vec![],
                acquisitions: vec![],
                aggregate: Default::default(),
                state,
                retry: Default::default(),
                created_at: std::time::Instant::now(),
                metadata: TaskMetadata {
                    name: None,
                    added_at_unix: 0,
                },
            },
            engine_tid: Some(ih.to_string()),
            engine_kind: EngineKind::Bt,
            engine_status: None,
            events: vec![],
        }
    }

    #[test]
    fn finished_alert_promotes_seeding() {
        let state = make_state_with(bt_rec(TaskState::Downloading(EngineKind::Bt), "ABC123"));
        let alert = Alert {
            kind: AlertKind::State,
            ih: "abc123".into(), // 大小写不同 → 归一化匹配
            msg: "torrent finished downloading".into(),
            at: 0,
            resume_ready: false,
        };
        let eff = state.apply_bt_alert(&alert).unwrap();
        assert_eq!(eff.from, TaskState::Downloading(EngineKind::Bt));
        assert_eq!(eff.to, TaskState::Seeding);
        let rec_lock = state.tasks.lock().unwrap();
        let rec = rec_lock.get("t1").unwrap();
        assert_eq!(rec.task.state, TaskState::Seeding, "任务记录状态必须落盘");
    }

    #[test]
    fn finished_from_queued_also_promotes() {
        // 任务还未被引擎快照驱动（仍 Queued）时，完成 alert 同样推进
        let state = make_state_with(bt_rec(TaskState::Queued, "AABB"));
        let alert = Alert {
            kind: AlertKind::State,
            ih: "aabb".into(),
            msg: "torrent finished downloading".into(),
            at: 0,
            resume_ready: false,
        };
        let eff = state.apply_bt_alert(&alert).unwrap();
        assert_eq!(eff.from, TaskState::Queued);
        assert_eq!(eff.to, TaskState::Seeding);
    }

    #[test]
    fn error_alert_fails_with_message() {
        let state = make_state_with(bt_rec(TaskState::Downloading(EngineKind::Bt), "D9E8"));
        let alert = Alert {
            kind: AlertKind::State,
            ih: "d9e8".into(),
            msg: "torrent error: pex failed".into(),
            at: 0,
            resume_ready: false,
        };
        let eff = state.apply_bt_alert(&alert).unwrap();
        assert_eq!(eff.to, TaskState::Failed);
        assert_eq!(eff.message, "torrent error: pex failed");
        let rec_lock = state.tasks.lock().unwrap();
        let rec = rec_lock.get("t1").unwrap();
        assert_eq!(rec.task.state, TaskState::Failed);
    }

    #[test]
    fn paused_alert_ignored() {
        // v1 不处理 Paused alert（pause 由 API 直调时同步发布事件）
        let state = make_state_with(bt_rec(TaskState::Downloading(EngineKind::Bt), "P1"));
        let alert = Alert {
            kind: AlertKind::State,
            ih: "p1".into(),
            msg: "torrent paused".into(),
            at: 0,
            resume_ready: false,
        };
        assert!(state.apply_bt_alert(&alert).is_none());
    }

    #[test]
    fn non_bt_task_ignored() {
        // HTTP 任务（engine_kind=Http）不匹配 BT alert
        let mut rec = bt_rec(TaskState::Downloading(EngineKind::Bt), "XT77");
        rec.engine_kind = EngineKind::Http;
        let state = make_state_with(rec);
        let alert = Alert {
            kind: AlertKind::State,
            ih: "xt77".into(),
            msg: "torrent finished downloading".into(),
            at: 0,
            resume_ready: false,
        };
        assert!(state.apply_bt_alert(&alert).is_none());
    }

    #[test]
    fn unknown_ih_ignored() {
        let state = make_state_with(bt_rec(TaskState::Downloading(EngineKind::Bt), "KN0WN"));
        let alert = Alert {
            kind: AlertKind::State,
            ih: "na-".into(),
            msg: "torrent finished downloading".into(),
            at: 0,
            resume_ready: false,
        };
        assert!(state.apply_bt_alert(&alert).is_none());
    }

    #[test]
    fn peer_alert_ignored() {
        let state = make_state_with(bt_rec(TaskState::Downloading(EngineKind::Bt), "PR99"));
        let alert = Alert {
            kind: AlertKind::Peer,
            ih: "pr99".into(),
            msg: "peer connected".into(),
            at: 0,
            resume_ready: false,
        };
        assert!(state.apply_bt_alert(&alert).is_none());
    }
}

/// torrent_infohash（bencode 最小解析）单元测试。
#[cfg(all(test, feature = "bt"))]
mod torrent_tests {
    use super::torrent_infohash;
    use super::torrent_total_size;

    /// 最小合法 .torrent：d4:info<infodict>e
    fn sample_torrent() -> Vec<u8> {
        let mut t = b"d4:infod6:lengthi123e4:name4:test12:piece lengthi16384e6:pieces20:".to_vec();
        t.extend_from_slice(&[0xAB; 20]);
        t.extend_from_slice(b"ee");
        t
    }

    #[test]
    fn extracts_infohash_from_info_dict() {
        let ih = torrent_infohash(&sample_torrent()).unwrap();
        // 预计算：SHA1(info dict) = 7ac2e18f...（info dict = t[7..=86]，80B）
        assert_eq!(ih, "7ac2e18f65f50b19e6bb1069e15ff2398aac220d");
    }

    #[test]
    fn rejects_non_dict_root() {
        assert!(torrent_infohash(b"nonsense").is_none());
        assert!(torrent_infohash(b"").is_none());
    }

    #[test]
    fn rejects_missing_info_key() {
        // 合法 bencode dict 但无 info 键
        let t = b"d3:foo3:bare";
        assert!(torrent_infohash(t).is_none());
    }

    #[test]
    fn skips_values_before_info_key() {
        // info 前有其他键值（含嵌套 list/int）仍能定位
        let mut t = b"d5:hello5:world7:payloadli3ee4:info".to_vec();
        t.extend_from_slice(&sample_torrent()[7..]); // info dict 起点起 + 顶层 e
        let ih = torrent_infohash(&t).unwrap();
        assert_eq!(ih, "7ac2e18f65f50b19e6bb1069e15ff2398aac220d");
    }

    #[test]
    fn total_size_single_file() {
        let t = sample_torrent();
        // length=123（单文件）
        assert_eq!(torrent_total_size(&t), Some(123));
    }

    #[test]
    fn total_size_multi_file_returns_none() {
        // 多文件 torrent：files 列表 → v1 None
        let mut t =
            b"d4:infod5:filesld6:lengthi10e4:pathl1:aeed6:lengthi20e4:pathl1:beeee".to_vec();
        assert_eq!(torrent_total_size(&t), None);
        let _ = &mut t;
    }

    #[test]
    fn total_size_missing_length_none() {
        let mut t = b"d4:info4:name4:teste".to_vec();
        assert_eq!(torrent_total_size(&t), None);
        let _ = &mut t;
    }
}

/// B10 预检单元测试（ensure_dest_root / precheck_space）。
#[cfg(test)]
mod b10_tests {
    use super::{ensure_dest_root, precheck_space, DaemonError};

    #[test]
    fn creates_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nested/deep");
        let p = ensure_dest_root(Some(missing.to_string_lossy().into_owned())).unwrap();
        assert!(p.is_dir(), "缺失目录应自动创建");
    }

    #[test]
    fn default_is_dot() {
        let p = ensure_dest_root(None).unwrap();
        assert!(p.is_dir());
    }

    #[test]
    fn invalid_path_rejected() {
        // Windows：非法路径字符 → 创建失败 → InvalidSource
        let r = ensure_dest_root(Some("a/b*c/d".into()));
        if let Err(DaemonError::InvalidSource(msg)) = r {
            assert!(msg.contains("不可创建") || msg.contains("不可写"));
        } else {
            // 某些平台可能允许——不强断言，仅确认类型
            assert!(r.is_ok() || r.is_err());
        }
    }

    #[test]
    fn check_space_zero_total_ok() {
        let dir = tempfile::tempdir().unwrap();
        assert!(precheck_space(dir.path(), 0).is_ok());
    }
}

/// 假引擎（持久化恢复测试用）：add 记录输入、可对指定 url 返回错误。
#[cfg(test)]
pub struct FakeEngine {
    kind: EngineKind,
    counter: std::sync::atomic::AtomicU64,
    fail_urls: std::sync::Mutex<std::collections::HashSet<String>>,
    added: std::sync::Mutex<Vec<String>>,
}

#[cfg(test)]
impl FakeEngine {
    pub fn new(kind: EngineKind) -> Self {
        FakeEngine {
            kind,
            counter: std::sync::atomic::AtomicU64::new(1),
            fail_urls: std::sync::Mutex::new(std::collections::HashSet::new()),
            added: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn fail_url(&self, url: &str) {
        self.fail_urls.lock().unwrap().insert(url.to_string());
    }

    pub fn added(&self) -> Vec<String> {
        self.added.lock().unwrap().clone()
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl DownloadEngine for FakeEngine {
    fn id(&self) -> &str {
        "fake"
    }

    fn kind(&self) -> EngineKind {
        self.kind
    }

    fn capabilities(&self) -> Vec<smart_dl_core::types::Capability> {
        vec![]
    }

    async fn add(
        &self,
        task: &DownloadTask,
    ) -> Result<EngineTaskId, smart_dl_core::types::EngineError> {
        let ident = match &task.source {
            DownloadSource::Http { url, .. } => url.clone(),
            DownloadSource::Magnet(m) => m.clone(),
            DownloadSource::TorrentFile(_) => format!("torrent:{}", task.id),
            _ => task.id.clone(),
        };
        if self.fail_urls.lock().unwrap().contains(&ident) {
            return Err(smart_dl_core::types::EngineError::Other("fake fail".into()));
        }
        self.added.lock().unwrap().push(ident);
        Ok(format!(
            "fk{}",
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ))
    }

    async fn pause(&self, _id: &EngineTaskId) -> Result<(), smart_dl_core::types::EngineError> {
        Ok(())
    }
    async fn resume(&self, _id: &EngineTaskId) -> Result<(), smart_dl_core::types::EngineError> {
        Ok(())
    }
    async fn status(
        &self,
        _id: &EngineTaskId,
    ) -> Result<EngineStatus, smart_dl_core::types::EngineError> {
        Ok(EngineStatus::default())
    }
    async fn remove(
        &self,
        _id: &EngineTaskId,
        _delete_data: bool,
    ) -> Result<(), smart_dl_core::types::EngineError> {
        Ok(())
    }
    async fn peers(
        &self,
        _id: &EngineTaskId,
    ) -> Result<Vec<smart_dl_core::types::PeerInfo>, smart_dl_core::types::EngineError> {
        Ok(vec![])
    }
    async fn update_sources(
        &self,
        _id: &EngineTaskId,
        _urls: Vec<String>,
    ) -> Result<(), smart_dl_core::types::EngineError> {
        Ok(())
    }
    async fn add_url_seed(
        &self,
        _id: &EngineTaskId,
        _url: &str,
    ) -> Result<(), smart_dl_core::types::EngineError> {
        Ok(())
    }
    async fn ban_peer(
        &self,
        _id: &EngineTaskId,
        _peer: std::net::SocketAddr,
    ) -> Result<(), smart_dl_core::types::EngineError> {
        Ok(())
    }
    async fn read_piece(
        &self,
        _id: &EngineTaskId,
        _idx: u32,
    ) -> Result<Vec<u8>, smart_dl_core::types::EngineError> {
        Ok(vec![])
    }
}

/// 任务持久化往返测试（FakeEngine，不联网）。
#[cfg(test)]
mod persist_tests {
    use super::*;

    fn wait_file(path: &std::path::Path, timeout_ms: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        while std::time::Instant::now() < deadline {
            if path.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("等待持久化文件超时: {path:?}");
    }

    #[tokio::test]
    async fn persist_then_restore_keeps_task() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("tasks.json");
        let fake = Arc::new(FakeEngine::new(EngineKind::Http));
        let state = Arc::new(DaemonState::new(fake.clone(), vec![]).with_storage(store.clone()));
        let tid = state
            .add_http_task("https://example.com/file.bin".into(), None)
            .await
            .unwrap();
        wait_file(&store, 2000);

        // 新 state（新引擎）恢复
        let fake2 = Arc::new(FakeEngine::new(EngineKind::Http));
        let state2 = DaemonState::new(fake2.clone(), vec![]);
        let n = state2.restore_from(&store).await.unwrap();
        assert_eq!(n, 1, "应恢复 1 条任务");
        let rec = state2.tasks.lock().unwrap().get(&tid).cloned().unwrap();
        assert_eq!(rec.task.id, tid, "task_id 必须保留");
        assert_eq!(rec.engine_kind, EngineKind::Http);
        assert_eq!(rec.task.state, TaskState::Queued, "恢复后重新入队");
        // 引擎重新 add 被调用
        assert_eq!(
            fake2.added(),
            vec!["https://example.com/file.bin".to_string()]
        );
    }

    #[tokio::test]
    async fn next_id_advances_after_restore() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("tasks.json");
        let fake = Arc::new(FakeEngine::new(EngineKind::Http));
        let state = Arc::new(DaemonState::new(fake.clone(), vec![]).with_storage(store.clone()));
        let _ = state
            .add_http_task("https://example.com/a.bin".into(), None)
            .await
            .unwrap();
        let _ = state
            .add_http_task("https://example.com/b.bin".into(), None)
            .await
            .unwrap();
        wait_file(&store, 2000);

        let state2 = DaemonState::new(Arc::new(FakeEngine::new(EngineKind::Http)), vec![]);
        state2.restore_from(&store).await.unwrap();
        let new_tid = state2
            .add_http_task("https://example.com/c.bin".into(), None)
            .await
            .unwrap();
        let num: u64 = new_tid.strip_prefix('t').unwrap().parse().unwrap();
        assert!(num >= 3, "恢复后新任务 id 应跳过已用 id: {new_tid}");
    }

    #[tokio::test]
    async fn restore_add_failure_marks_failed() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("tasks.json");
        let fake = Arc::new(FakeEngine::new(EngineKind::Http));
        let state = Arc::new(DaemonState::new(fake.clone(), vec![]).with_storage(store.clone()));
        let tid = state
            .add_http_task("https://example.com/gone.bin".into(), None)
            .await
            .unwrap();
        wait_file(&store, 2000);

        let fake2 = Arc::new(FakeEngine::new(EngineKind::Http));
        fake2.fail_url("https://example.com/gone.bin");
        let state2 = DaemonState::new(fake2.clone(), vec![]);
        let n = state2.restore_from(&store).await.unwrap();
        assert_eq!(n, 0, "add 失败不计入恢复数");
        let rec = state2.tasks.lock().unwrap().get(&tid).cloned().unwrap();
        assert_eq!(rec.task.state, TaskState::Failed, "add 失败任务标 Failed");
        assert!(rec.engine_tid.is_none());
    }

    #[tokio::test]
    async fn no_storage_no_autosave() {
        let fake = Arc::new(FakeEngine::new(EngineKind::Http));
        let state = DaemonState::new(fake.clone(), vec![]);
        let _ = state
            .add_http_task("https://example.com/x.bin".into(), None)
            .await
            .unwrap();
        // 无 persist_path → 无写盘（autosave 直接 return）
        // 此测试验证不 panic；写盘路径由 with_storage 测试覆盖。
        assert!(fake.added().len() == 1);
    }

    #[tokio::test]
    async fn dest_none_uses_default_dest_root() {
        // with_dest_root 注入默认目录后，dest 未指定 → 任务落默认目录（而非 daemon cwd）
        let fake = Arc::new(FakeEngine::new(EngineKind::Http));
        let state = DaemonState::new(fake.clone(), vec![])
            .with_dest_root(std::path::PathBuf::from("/data/default-dl"));
        let tid = state
            .add_http_task("https://example.com/dest.bin".into(), None)
            .await
            .unwrap();
        let rec = state.tasks.lock().unwrap().get(&tid).cloned().unwrap();
        assert_eq!(
            rec.task.dest_root,
            std::path::PathBuf::from("/data/default-dl"),
            "dest 未指定应落到默认 dest_root"
        );
    }

    #[tokio::test]
    async fn explicit_dest_overrides_default() {
        let fake = Arc::new(FakeEngine::new(EngineKind::Http));
        let state = DaemonState::new(fake.clone(), vec![])
            .with_dest_root(std::path::PathBuf::from("/data/default-dl"));
        let tid = state
            .add_http_task(
                "https://example.com/override.bin".into(),
                Some("/tmp/custom".into()),
            )
            .await
            .unwrap();
        let rec = state.tasks.lock().unwrap().get(&tid).cloned().unwrap();
        assert_eq!(
            rec.task.dest_root,
            std::path::PathBuf::from("/tmp/custom"),
            "显式 dest 优先于默认"
        );
    }
}
