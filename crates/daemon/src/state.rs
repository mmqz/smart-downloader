//! DaemonState（M6 集成层）：任务目录 + HttpEngine + FallbackPolicy + WsHub；
//! add/pause/resume/remove/snapshot/list/provider 快照；重复 canonical → 409 事件。

use smart_dl_core::identity::{CanonicalId, CanonicalKind, ContentIdentity};
use smart_dl_core::state_machine::TaskState;
use smart_dl_core::task::{DownloadTask, TaskId, TaskMetadata};
use smart_dl_core::types::{
    DownloadEngine, DownloadSource, EngineKind, EngineStatus, EngineTaskId,
};
use smart_dl_httpdl::HttpEngine;
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

/// 守护进程状态：任务 + 引擎 + 事件中枢。
pub struct DaemonState {
    engine: HttpEngine,
    hub: WsHub,
    tasks: Mutex<HashMap<TaskId, TaskRecord>>,
    providers: Vec<Arc<dyn RemoteProvider>>,
    next_id: AtomicU64,
}

impl DaemonState {
    pub fn new(engine: HttpEngine, providers: Vec<Arc<dyn RemoteProvider>>) -> Self {
        DaemonState {
            engine,
            hub: WsHub::new(),
            tasks: Mutex::new(HashMap::new()),
            providers,
            next_id: AtomicU64::new(1),
        }
    }

    pub fn hub(&self) -> &WsHub {
        &self.hub
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
            identity: url.clone(), // v1：URL 字符串为 canonical（D34 token 剔除 v2）
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
            .engine
            .add(&task)
            .await
            .map_err(|e| DaemonError::Engine(e.to_string()))?;
        self.tasks.lock().unwrap().insert(
            task_id.clone(),
            TaskRecord {
                task,
                engine_tid: Some(engine_tid),
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
        let (engine, status) = match &rec.engine_tid {
            Some(tid) => {
                let st = self.engine.status(tid).await.ok();
                (Some("http".to_string()), st)
            }
            None => (None, None),
        };
        Some(TaskSnapshot {
            task_id: id.to_string(),
            state: rec.task.state.clone(),
            source: format!("{:?}", rec.task.source),
            dest_root: rec.task.dest_root.clone(),
            engine,
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
        let tid = self
            .tasks
            .lock()
            .unwrap()
            .get(id)
            .and_then(|r| r.engine_tid.clone())
            .ok_or_else(|| DaemonError::NotFound(id.to_string()))?;
        self.engine
            .pause(&tid)
            .await
            .map_err(|e| DaemonError::Engine(e.to_string()))?;
        self.hub.publish(SchedulerEvent::StateChanged {
            task_id: id.to_string(),
            from: TaskState::Downloading(EngineKind::Http),
            to: TaskState::Paused,
        });
        Ok(())
    }

    pub async fn resume(&self, id: &str) -> Result<(), DaemonError> {
        let tid = self
            .tasks
            .lock()
            .unwrap()
            .get(id)
            .and_then(|r| r.engine_tid.clone())
            .ok_or_else(|| DaemonError::NotFound(id.to_string()))?;
        self.engine
            .resume(&tid)
            .await
            .map_err(|e| DaemonError::Engine(e.to_string()))?;
        self.hub.publish(SchedulerEvent::StateChanged {
            task_id: id.to_string(),
            from: TaskState::Paused,
            to: TaskState::Downloading(EngineKind::Http),
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
            let _ = self.engine.remove(&tid, false).await;
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
}
