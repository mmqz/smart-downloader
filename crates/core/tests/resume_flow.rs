//! M3: resume 全流程（§12 D16 恢复）。
//! resume.bencode 保存/加载 + 损坏处理（报错不崩溃重建）+ 保存时机策略。

mod common;

use smart_dl_core::session::manager::{should_save, ResumeOutcome, SaveReason, SessionManager};
use std::fs;
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_secs(600); // 10 分钟

#[test]
fn resume_roundtrip_bytes_identical() {
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    let data = b"\x64\x38\x3a\x66\x69\x6c\x65\x73\x6c\x65"; // bencode 风格片段
    m.save_resume("r1", data).unwrap();
    match m.load_resume("r1") {
        ResumeOutcome::Ok(v) => assert_eq!(v, data),
        other => panic!("期望 Ok，得到 {other:?}"),
    }
}

#[test]
fn load_resume_missing_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    assert!(matches!(m.load_resume("nope"), ResumeOutcome::Missing));
}

#[test]
fn corrupted_resume_reports_error_but_task_survives() {
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    // 模拟磁盘上残留的损坏 resume（校验失败而非崩溃）
    fs::create_dir_all(m.resume_path("r3").parent().unwrap()).unwrap();
    fs::write(m.resume_path("r3"), b"\xff\xfe not bencode").unwrap();

    match m.load_resume("r3") {
        ResumeOutcome::Corrupted(msg) => assert!(!msg.is_empty()),
        other => panic!("期望 Corrupted，得到 {other:?}"),
    }

    // 任务状态文件不受影响：仍可加载任务
    let task = common::make_task("r3", "survive");
    m.save_task(&task).unwrap();
    assert!(m.load_task("r3").is_loaded());
}

#[test]
fn pause_complete_shutdown_always_save() {
    let now = Instant::now();
    for reason in [
        SaveReason::Pause,
        SaveReason::Complete,
        SaveReason::Shutdown,
    ] {
        assert!(should_save(now, reason, INTERVAL), "{reason:?} 必须保存");
    }
}

#[test]
fn periodic_respects_interval() {
    let start = Instant::now();
    // 刚保存过 → 不到 10 分钟 → 不保存
    assert!(!should_save(start, SaveReason::Periodic, INTERVAL));
    // 超过 10 分钟 → 保存
    let late = start - Duration::from_secs(601);
    assert!(should_save(late, SaveReason::Periodic, INTERVAL));
}
