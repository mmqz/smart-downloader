//! 会话管理（§12）：`~/.config/smart-dl/sessions/<task_uuid>/` 下的 state.json 与 resume.bencode。
//! - 原子写：state.json.tmp + rename（崩溃时残留 tmp 被忽略，正式文件要么旧、要么新）
//! - 恢复（D16）：暂停/完成/退出 + 每 10 分钟触发保存；resume 损坏 → Corrupted（任务可重建，不崩溃）

use crate::task::DownloadTask;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 会话管理器：目录布局 `<session_dir>/<task_id>/state.json(+.tmp) / resume.bencode`。
#[derive(Clone, Debug)]
pub struct SessionManager {
    session_dir: PathBuf,
}

impl SessionManager {
    pub fn new(session_dir: PathBuf) -> Self {
        SessionManager { session_dir }
    }

    /// `<session_dir>/<task_id>/`
    pub fn task_dir(&self, task_id: &str) -> PathBuf {
        self.session_dir.join(task_id)
    }

    /// state.json 路径（原子写的正式文件）。
    pub fn state_path(&self, task_id: &str) -> PathBuf {
        self.task_dir(task_id).join("state.json")
    }

    /// resume.bencode 路径（M6 交给 libtorrent 的恢复数据）。
    pub fn resume_path(&self, task_id: &str) -> PathBuf {
        self.task_dir(task_id).join("resume.bencode")
    }

    /// 原子写任务状态：先写 `.tmp` 再 rename。
    pub fn save_task(&self, task: &DownloadTask) -> Result<(), SessionError> {
        let dir = self.task_dir(&task.id);
        fs::create_dir_all(&dir)?;
        let final_path = dir.join("state.json");
        let tmp_path = dir.join("state.json.tmp");
        let bytes = serde_json::to_vec_pretty(task)?;
        fs::write(&tmp_path, bytes)?;
        fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    /// 加载任务状态。写一半/损坏 → Corrupted（调用方忽略重建）；残留 tmp 不影响。
    pub fn load_task(&self, task_id: &str) -> LoadOutcome {
        let p = self.state_path(task_id);
        let bytes = match fs::read(&p) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LoadOutcome::Missing,
            Err(e) => return LoadOutcome::Corrupted(e.to_string()),
        };
        match serde_json::from_slice::<DownloadTask>(&bytes) {
            Ok(t) => LoadOutcome::Loaded(Box::new(t)),
            Err(e) => LoadOutcome::Corrupted(e.to_string()),
        }
    }

    /// 删除任务会话目录（含 state/resume/.part 保留由调用方决定——此处仅状态）。
    pub fn delete_task(&self, task_id: &str) -> Result<(), SessionError> {
        let dir = self.task_dir(task_id);
        if !dir.exists() {
            return Err(SessionError::NotFound(task_id.to_string()));
        }
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    /// 保存 resume 数据（D16：暂停/完成/退出/10min 时由调用方取 libtorrent resume 写入）。
    pub fn save_resume(&self, task_id: &str, data: &[u8]) -> Result<(), SessionError> {
        let p = self.resume_path(task_id);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&p, data)?;
        Ok(())
    }

    /// 加载 resume。损坏（非 bencode dict 首字节）→ Corrupted；缺失 → Missing。
    /// 注：M3 只做轻量结构校验（bencode dict 首字节），完整解析由 M6 交给 libtorrent。
    pub fn load_resume(&self, task_id: &str) -> ResumeOutcome {
        let p = self.resume_path(task_id);
        let bytes = match fs::read(&p) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ResumeOutcome::Missing,
            Err(e) => return ResumeOutcome::Corrupted(e.to_string()),
        };
        if bytes.first() == Some(&b'd') {
            ResumeOutcome::Ok(bytes)
        } else {
            ResumeOutcome::Corrupted("resume 不是 bencode dict".to_string())
        }
    }
}

/// state.json 加载结果。
#[derive(Debug)]
pub enum LoadOutcome {
    Loaded(Box<DownloadTask>),
    Missing,
    /// 文件损坏（含写一半的崩溃产物）——调用方应忽略并重建。
    Corrupted(String),
}

impl LoadOutcome {
    pub fn is_loaded(&self) -> bool {
        matches!(self, LoadOutcome::Loaded(_))
    }
}

/// resume.bencode 加载结果。
#[derive(Debug)]
pub enum ResumeOutcome {
    Ok(Vec<u8>),
    Missing,
    Corrupted(String),
}

/// 保存时机（D16）。Pause/Complete/Shutdown 必须保存；Periodic 按间隔。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveReason {
    Pause,
    Complete,
    Shutdown,
    Periodic,
}

/// 是否应触发保存。
pub fn should_save(last_saved: Instant, reason: SaveReason, interval: Duration) -> bool {
    match reason {
        SaveReason::Periodic => last_saved.elapsed() >= interval,
        _ => true,
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SessionError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("task not found: {0}")]
    NotFound(String),
}