//! M2: 并发队列（§3 并发配额 D24：BT≤3 / HTTP·FTP≤8 / Provider≤2，超出进 Queued FIFO）。

use smart_dl_core::registry::{EngineRegistry, QueueOutcome, TaskQueue};
use smart_dl_core::types::EngineKind;
use std::sync::Arc;

use mocks::mock_engine::MockEngine;
mod mocks;

#[test]
fn default_quotas_frozen() {
    let reg = EngineRegistry::new();
    assert_eq!(reg.quota(EngineKind::Bt), 3);
    assert_eq!(reg.quota(EngineKind::Http), 8);
    assert_eq!(reg.quota(EngineKind::Ftp), 8);
    assert_eq!(reg.quota(EngineKind::Provider), 2);
}

#[test]
fn bt_quota_three_fourth_queued() {
    let mut q = TaskQueue::default();
    let mut out = Vec::new();
    for i in 0..4 {
        out.push(q.submit(format!("t{i}"), EngineKind::Bt));
    }
    assert_eq!(out, vec![
        QueueOutcome::Started,
        QueueOutcome::Started,
        QueueOutcome::Started,
        QueueOutcome::Queued,
    ]);
    assert_eq!(q.active_count(EngineKind::Bt), 3);
}

#[test]
fn http_quota_eight_ninth_queued() {
    let mut q = TaskQueue::default();
    let ninth = (0..9).map(|i| q.submit(format!("h{i}"), EngineKind::Http)).last().unwrap();
    assert_eq!(ninth, QueueOutcome::Queued);
    assert_eq!(q.active_count(EngineKind::Http), 8);
}

#[test]
fn release_starts_fifo_head() {
    let mut q = TaskQueue::default();
    q.submit("bt1".into(), EngineKind::Bt);
    q.submit("bt2".into(), EngineKind::Bt);
    q.submit("bt3".into(), EngineKind::Bt);
    q.submit("bt4".into(), EngineKind::Bt); // queued
    q.submit("bt5".into(), EngineKind::Bt); // queued
    assert_eq!(q.waiting_len(), 2);
    // 释放一个 → 最早入队（bt4）被启动
    let next = q.release(EngineKind::Bt);
    assert_eq!(next.as_deref(), Some("bt4"));
    assert_eq!(q.waiting_len(), 1);
    let next = q.release(EngineKind::Bt);
    assert_eq!(next.as_deref(), Some("bt5"));
    assert_eq!(q.waiting_len(), 0);
    // 无等待任务 → None
    assert_eq!(q.release(EngineKind::Bt), None);
}

#[test]
fn kinds_quota_independent() {
    let mut q = TaskQueue::default();
    for i in 0..3 {
        q.submit(format!("b{i}"), EngineKind::Bt);
    }
    for i in 0..4 {
        q.submit(format!("h{i}"), EngineKind::Http);
    }
    q.submit("b3".into(), EngineKind::Bt); // queued (bt 满)
    q.submit("h4".into(), EngineKind::Http); // started (http 未满)
    assert_eq!(q.active_count(EngineKind::Bt), 3);
    assert_eq!(q.active_count(EngineKind::Http), 5);
    assert_eq!(q.waiting_len(), 1);
}

#[test]
fn registry_surface_quota_passthrough() {
    let reg = EngineRegistry::new();
    assert_eq!(reg.quota(EngineKind::Bt), TaskQueue::default().quota(EngineKind::Bt));
}

#[test]
fn provider_quota_two() {
    let mut q = TaskQueue::default();
    q.submit("p1".into(), EngineKind::Provider);
    q.submit("p2".into(), EngineKind::Provider);
    assert_eq!(q.submit("p3".into(), EngineKind::Provider), QueueOutcome::Queued);
}

#[test]
fn register_and_get_roundtrip() {
    let mut reg = EngineRegistry::new();
    let bt = Arc::new(MockEngine::bt());
    reg.register(bt.clone()).unwrap();
    let got = reg.get("bt").expect("engine present");
    assert_eq!(got.id(), "bt");
    assert!(reg.get("nope").is_none());
}