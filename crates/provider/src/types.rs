//! Provider 类型（§13）：运行态、任务状态、直链文件、错误。

use serde::{Deserialize, Serialize};

/// Provider 侧任务句柄（v1 用字符串 id）。
pub type ProviderTaskId = String;

/// Provider 运行态（D5：Provider 含运行态，供路由选择）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRuntime {
    pub enabled: bool,
    pub authenticated: bool,
    /// 剩余配额（不自动烧配额：quota>0 才可选）。
    pub quota_remaining: u64,
    /// 并发上限（D24：Provider ≤2）。
    pub concurrency_limit: u32,
    /// 当前占用并发数。
    pub busy: u32,
    /// 冷却到 unix 秒（backoff 中不可选）。
    pub backoff_until: Option<u64>,
    pub last_error: Option<String>,
}

impl Default for ProviderRuntime {
    fn default() -> Self {
        ProviderRuntime {
            enabled: true,
            authenticated: true,
            quota_remaining: u64::MAX,
            concurrency_limit: 2,
            busy: 0,
            backoff_until: None,
            last_error: None,
        }
    }
}

/// Provider 任务状态（submit → 轮询 → resolve）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ProviderStatus {
    #[default]
    Queued,
    Downloading,
    Ready,
    Failed,
}

/// 直链文件（§13）：resolve 返回，HttpEngine 承接下载。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolvedRemoteFile {
    pub rel_path: String,
    pub url: String,
    pub size: u64,
    pub etag: Option<String>,
    /// 过期 unix 秒；None = 不过期。
    pub expires_at: Option<u64>,
}

/// Provider 错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("provider auth failed")]
    Auth,
    #[error("provider quota exhausted")]
    Quota,
    #[error("provider task not found")]
    NotFound,
    #[error("direct link expired")]
    Expired,
    #[error("no provider available")]
    NoProvider,
    #[error("auto fallback rejected (manual only)")]
    ManualOnly,
    #[error("provider must start after bt pause")]
    RequiresPause,
    #[error("retries exhausted")]
    RetriesExhausted,
    #[error("provider error: {0}")]
    Other(String),
}

/// 当前 unix 秒（可注入时钟的替身：测试直接构造过期时间）。
pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// 直链是否已过期。
pub(crate) fn link_expired(file: &ResolvedRemoteFile, now: u64) -> bool {
    file.expires_at.map(|e| e < now).unwrap_or(false)
}
