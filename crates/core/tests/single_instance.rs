//! M3: 单实例锁（§12 D24）。
//! lock 已存在 → AlreadyRunning（daemon 转发任务后退出）；释放后可再获取。

use smart_dl_core::session::single_instance::{InstanceLock, LockStatus};

#[test]
fn acquire_new_lock_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("lock");
    let lock = InstanceLock::acquire(&p);
    assert!(matches!(lock, LockStatus::Acquired(_)));
    assert!(p.exists());
}

#[test]
fn second_acquire_reports_already_running() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("lock");
    let lock = InstanceLock::acquire(&p);
    assert!(matches!(lock, LockStatus::Acquired(_)));

    let second = InstanceLock::acquire(&p);
    assert!(matches!(second, LockStatus::AlreadyRunning));
}

#[test]
fn release_then_reacquire_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("lock");
    let lock = InstanceLock::acquire(&p);
    match &lock {
        LockStatus::Acquired(_) => lock.release(),
        LockStatus::AlreadyRunning => panic!("首个 acquire 不可能 AlreadyRunning"),
    }
    assert!(!p.exists());

    let again = InstanceLock::acquire(&p);
    assert!(matches!(again, LockStatus::Acquired(_)));
}