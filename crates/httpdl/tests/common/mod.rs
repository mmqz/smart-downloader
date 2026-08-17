//! httpdl 测试共享：构造 Http 任务（最小 DownloadTask）。

use smart_dl_core::identity::{CanonicalId, CanonicalKind, ContentIdentity};
use smart_dl_core::state_machine::{EvalPhase, TaskState};
use smart_dl_core::task::{DownloadTask, ProgressAggregate, RetryState, TaskMetadata};
use smart_dl_core::types::DownloadSource;
use std::path::PathBuf;
use std::time::Instant;

pub fn make_http_task(id: &str, url: &str) -> DownloadTask {
    DownloadTask {
        id: id.to_string(),
        canonical_id: CanonicalId {
            kind: CanonicalKind::Http,
            identity: url.to_string(),
            validator: None,
            token_sensitive: false,
        },
        source: DownloadSource::Http {
            url: url.to_string(),
            headers: vec![],
            auth: None,
        },
        identity: ContentIdentity::SingleFile {
            size: 0,
            etag: None,
            sha256: None,
        },
        dest_root: PathBuf::from("."),
        files: vec![],
        acquisitions: vec![],
        aggregate: ProgressAggregate::default(),
        state: TaskState::Evaluating(EvalPhase::MetadataPending),
        retry: RetryState::default(),
        created_at: Instant::now(),
        metadata: TaskMetadata::default(),
    }
}