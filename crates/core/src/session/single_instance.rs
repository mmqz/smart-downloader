//! 单实例锁（§12 D24）：lock 文件已存在 → 新实例转发任务后退出。
//! 实现：`create_new` 原子创建；进程退出/显式 release 时删除。

use std::fs;
use std::path::{Path, PathBuf};

/// 锁结果：Acquired 携带 lock 文件路径（release 时用）。
#[derive(Debug)]
pub enum LockStatus {
    Acquired(PathBuf),
    AlreadyRunning,
}

impl LockStatus {
    /// 释放锁（删除 lock 文件）。非 Acquired 时无操作。
    pub fn release(&self) {
        if let LockStatus::Acquired(p) = self {
            let _ = fs::remove_file(p);
        }
    }
}

/// 单实例锁。`acquire` 原子创建 lock 文件：
/// - 成功 → Acquired（唯一运行实例）
/// - 已存在 → AlreadyRunning（调用方应转发任务后退出，§12 D24）
pub struct InstanceLock;

impl InstanceLock {
    pub fn acquire(path: &Path) -> LockStatus {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(_) => LockStatus::Acquired(path.to_path_buf()),
            Err(_) => LockStatus::AlreadyRunning,
        }
    }
}