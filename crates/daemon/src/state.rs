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
use std::path::PathBuf;
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
}

/// 任务快照（GET /tasks/:id，跳号补拉入口）。
#[derive(Clone, Debug, serde::Serialize)]
pub struct TaskSnapshot {
    pub task_id: String,
    pub state: TaskState,
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
    pub state: TaskState,
    pub source: String,
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
}

/// 守护进程状态：任务 + 引擎表 + 事件中枢。
pub struct DaemonState {
    engines: HashMap<EngineKind, Arc<dyn DownloadEngine>>,
    hub: WsHub,
    tasks: Mutex<HashMap<TaskId, TaskRecord>>,
    providers: Vec<Arc<dyn RemoteProvider>>,
    next_id: AtomicU64,
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
        }
    }

    /// 追加 BT 引擎（feature `bt`；无该引擎时 magnet 路由 → InvalidSource）。
    #[cfg(feature = "bt")]
    pub fn with_bt(mut self, bt: Arc<dyn DownloadEngine>) -> Self {
        self.engines.insert(EngineKind::Bt, bt);
        self
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
            dest_root: dest_root
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".")),
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
        self.tasks.lock().unwrap().insert(
            task_id.clone(),
            TaskRecord {
                task,
                engine_tid: Some(engine_tid),
                engine_kind: EngineKind::Bt,
                engine_status: None,
            },
        );
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
        let Some(ih) = torrent_infohash(&torrent_bytes) else {
            return Err(DaemonError::InvalidSource(
                ".torrent 解析失败：无法定位 info dict".into(),
            ));
        };
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
            dest_root: dest_root
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".")),
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
        self.tasks.lock().unwrap().insert(
            task_id.clone(),
            TaskRecord {
                task,
                engine_tid: Some(engine_tid),
                engine_kind: EngineKind::Bt,
                engine_status: None,
            },
        );
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
            dest_root: dest_root
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".")),
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
        self.tasks.lock().unwrap().insert(
            task_id.clone(),
            TaskRecord {
                task,
                engine_tid: Some(engine_tid),
                engine_kind: EngineKind::Http,
                engine_status: None,
            },
        );
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
            Some(s) => engine_state_to_task(&s.state, rec.engine_kind),
            None => rec.task.state.clone(),
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
                state: rec.task.state.clone(),
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
        Ok(())
    }

    /// Provider 运行态快照（健康/配额/冷却）。
    pub fn provider_status(&self) -> Vec<(String, ProviderRuntime)> {
        self.providers
            .iter()
            .map(|p| (p.name().to_string(), p.runtime()))
            .collect()
    }

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
            return Some(BtAlertEffect {
                task_id: id.clone(),
                from,
                to,
                message: a.msg.clone(),
            });
        }
        None
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
fn btih_of(magnet: &str) -> Option<String> {
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
            let digest = sha1::Sha1::digest(&b[i..=end]);
            return Some(
                digest
                    .iter()
                    .map(|x| format!("{x:02x}"))
                    .collect::<String>(),
            );
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
}
