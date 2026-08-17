//! 任务与文件模型（§7）。

use crate::identity::{CanonicalId, ContentIdentity};
use crate::ownership::Acquisition;
use crate::state_machine::TaskState;
use crate::types::{DownloadSource, EngineKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 任务 id（v1 字符串句柄）。
pub type TaskId = String;

/// 任务（§7）。数据所有权：Task 拥有 files[]；引擎只有传输权（§9）。
#[derive(Clone, Debug)]
pub struct DownloadTask {
    pub id: TaskId,
    pub canonical_id: CanonicalId,
    pub source: DownloadSource,
    pub identity: ContentIdentity,
    pub dest_root: PathBuf,
    pub files: Vec<TaskFile>,
    pub acquisitions: Vec<Acquisition>,
    pub aggregate: ProgressAggregate,
    pub state: TaskState,
    pub retry: RetryState,
    pub created_at: std::time::Instant,
    pub metadata: TaskMetadata,
}

/// 单文件（§7）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskFile {
    pub rel_path: String,
    pub size: u64,
    pub done: u64,
    pub state: FileState,
    pub source_urls: Vec<String>,
    pub identity: Option<ContentIdentity>,
    pub etag: Option<String>,
    pub engine: EngineKind,
}

/// 文件级状态（§15 文件级进度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum FileState {
    #[default]
    Pending,
    Active,
    Done,
}

/// 聚合进度。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgressAggregate {
    pub done: u64,
    pub total: u64,
}

/// 重试状态（§10 重试超上限 → Failed）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RetryState {
    pub retries: u32,
    pub max_retries: u32,
}

/// 任务元数据（附加信息，字段随 M3 会话持久化扩充）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskMetadata {
    pub name: Option<String>,
    pub added_at_unix: u64,
}