//! M3: 会话持久化 roundtrip（§12 会话目录）。
//! state.json 原子写（tmp+rename）+ 加载 + 崩溃恢复（写一半/残留 tmp 不炸）。

mod common;

use common::make_task;
use smart_dl_core::session::manager::{LoadOutcome, SessionManager};
use smart_dl_core::state_machine::{EvalPhase, TaskState};
use std::fs;

#[test]
fn save_then_load_roundtrip_fields_match() {
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    let task = make_task("t1", "roundtrip");

    m.save_task(&task).unwrap();
    let loaded = match m.load_task("t1") {
        LoadOutcome::Loaded(t) => t,
        other => panic!("期望 Loaded，得到 {other:?}"),
    };

    assert_eq!(loaded.id, task.id);
    assert_eq!(loaded.canonical_id, task.canonical_id);
    assert_eq!(loaded.source, task.source);
    assert_eq!(loaded.files, task.files);
    assert_eq!(loaded.acquisitions, task.acquisitions);
    assert_eq!(loaded.aggregate, task.aggregate);
    assert_eq!(loaded.state, task.state);
    assert_eq!(loaded.retry, task.retry);
    assert_eq!(loaded.metadata, task.metadata);
}

#[test]
fn load_missing_task_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    assert!(matches!(m.load_task("nope"), LoadOutcome::Missing));
}

#[test]
fn corrupted_state_json_reports_corrupted_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    let p = m.state_path("t2");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    // 写一半的 JSON（截断 = 崩溃瞬间产物）
    fs::write(&p, br#"{"id":"t2","canonical_id":{"kind":"Bt","#).unwrap();

    match m.load_task("t2") {
        LoadOutcome::Corrupted(msg) => assert!(!msg.is_empty()),
        other => panic!("期望 Corrupted，得到 {other:?}"),
    }
}

#[test]
fn stale_tmp_from_crash_is_ignored_and_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    // 原子写崩溃：tmp 残留、正式文件缺失 → 视作 Missing（忽略 tmp），可重建
    let tdir = m.task_dir("t3");
    fs::create_dir_all(&tdir).unwrap();
    fs::write(tdir.join("state.json.tmp"), b"partial").unwrap();

    assert!(matches!(m.load_task("t3"), LoadOutcome::Missing));

    // 重建成功：新 save 覆盖一切
    let task = make_task("t3", "recover");
    m.save_task(&task).unwrap();
    assert!(matches!(m.load_task("t3"), LoadOutcome::Loaded(_)));
}

#[test]
fn delete_task_removes_state() {
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    let task = make_task("t4", "del");
    m.save_task(&task).unwrap();
    m.delete_task("t4").unwrap();
    assert!(matches!(m.load_task("t4"), LoadOutcome::Missing));
}

#[test]
fn load_task_in_wrong_state_keeps_transition_valid() {
    // 序列化往返后 TaskState 仍是合法枚举（serde 往返不破坏状态机前提）
    let dir = tempfile::tempdir().unwrap();
    let m = SessionManager::new(dir.path().to_path_buf());
    let task = make_task("t5", "state");
    m.save_task(&task).unwrap();
    let loaded = match m.load_task("t5") {
        LoadOutcome::Loaded(t) => t,
        other => panic!("{other:?}"),
    };
    assert!(matches!(
        loaded.state,
        TaskState::Evaluating(EvalPhase::MetadataPending)
    ));
}
