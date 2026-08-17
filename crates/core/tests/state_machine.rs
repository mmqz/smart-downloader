//! M2: 状态机完整流转矩阵。
//! Queued→Evaluating→(MetadataPending→PeerDiscovery→HeatEvaluating)→Downloading→Completed→Stopped
//! Downloading→(stall)→Paused / FallbackProvider→Transferring→Completed
//! *→Failed；非法转换拒绝；Completed 不直接进 Seeding（默认）。

use smart_dl_core::ownership::FallbackPolicy;
use smart_dl_core::state_machine::{EvalPhase, StateMachine, TaskState, TransitionCtx};
use smart_dl_core::types::EngineKind;

fn ctx() -> TransitionCtx {
    TransitionCtx::default()
}

fn ctx_metadata() -> TransitionCtx {
    TransitionCtx {
        metadata_received: true,
        ..ctx()
    }
}

fn ctx_heat(score: f64) -> TransitionCtx {
    TransitionCtx {
        heat: Some(score),
        ..ctx()
    }
}

fn ctx_stalled(progress: f64) -> TransitionCtx {
    TransitionCtx {
        stalled: true,
        bt_progress: progress,
        ..ctx()
    }
}

#[test]
fn queued_to_evaluating_requires_quota() {
    let sm = StateMachine;
    assert!(sm.can_transition(
        &TaskState::Queued,
        &TaskState::Evaluating(EvalPhase::MetadataPending),
        &ctx()
    ));
    let mut no_quota = ctx();
    no_quota.quota_ok = false;
    assert!(!sm.can_transition(
        &TaskState::Queued,
        &TaskState::Evaluating(EvalPhase::MetadataPending),
        &no_quota
    ));
}

#[test]
fn metadata_pending_to_peer_discovery_on_metadata() {
    let sm = StateMachine;
    assert!(sm.can_transition(
        &TaskState::Evaluating(EvalPhase::MetadataPending),
        &TaskState::Evaluating(EvalPhase::PeerDiscovery),
        &ctx_metadata()
    ));
    assert!(!sm.can_transition(
        &TaskState::Evaluating(EvalPhase::MetadataPending),
        &TaskState::Evaluating(EvalPhase::PeerDiscovery),
        &ctx()
    ));
}

#[test]
fn peer_discovery_to_heat_evaluating() {
    let sm = StateMachine;
    assert!(sm.can_transition(
        &TaskState::Evaluating(EvalPhase::PeerDiscovery),
        &TaskState::Evaluating(EvalPhase::HeatEvaluating),
        &ctx()
    ));
}

#[test]
fn heat_evaluating_hot_to_downloading_bt() {
    let sm = StateMachine;
    assert!(sm.can_transition(
        &TaskState::Evaluating(EvalPhase::HeatEvaluating),
        &TaskState::Downloading(EngineKind::Bt),
        &ctx_heat(0.7)
    ));
    // 冷 → 不给 BT（路由到兜底，见 router/fallback 测试）
    assert!(!sm.can_transition(
        &TaskState::Evaluating(EvalPhase::HeatEvaluating),
        &TaskState::Downloading(EngineKind::Bt),
        &ctx_heat(0.2)
    ));
}

#[test]
fn heat_evaluating_cold_to_fallback_provider() {
    let sm = StateMachine;
    assert!(sm.can_transition(
        &TaskState::Evaluating(EvalPhase::HeatEvaluating),
        &TaskState::FallbackProvider,
        &ctx_heat(0.2)
    ));
    assert!(!sm.can_transition(
        &TaskState::Evaluating(EvalPhase::HeatEvaluating),
        &TaskState::FallbackProvider,
        &ctx_heat(0.7)
    ));
}

#[test]
fn downloading_stalled_to_paused() {
    let sm = StateMachine;
    // 必须经过 stall（30s 无进展）才能暂停
    assert!(sm.can_transition(
        &TaskState::Downloading(EngineKind::Bt),
        &TaskState::Paused,
        &ctx_stalled(0.8)
    ));
    assert!(!sm.can_transition(
        &TaskState::Downloading(EngineKind::Bt),
        &TaskState::Paused,
        &ctx()
    ));
}

#[test]
fn downloading_stalled_low_progress_to_fallback() {
    let sm = StateMachine;
    // BT <50% → 允许串行兜底
    assert!(sm.can_transition(
        &TaskState::Downloading(EngineKind::Bt),
        &TaskState::FallbackProvider,
        &ctx_stalled(0.4)
    ));
    // BT ≥50% → 拒绝兜底（保持可恢复暂停）
    assert!(!sm.can_transition(
        &TaskState::Downloading(EngineKind::Bt),
        &TaskState::FallbackProvider,
        &ctx_stalled(0.6)
    ));
}

#[test]
fn fallback_to_transferring_to_completed() {
    let sm = StateMachine;
    assert!(sm.can_transition(
        &TaskState::FallbackProvider,
        &TaskState::Transferring,
        &ctx()
    ));
    assert!(sm.can_transition(&TaskState::Transferring, &TaskState::Completed, &ctx()));
    assert!(sm.can_transition(
        &TaskState::Downloading(EngineKind::Http),
        &TaskState::Completed,
        &ctx()
    ));
}

#[test]
fn completed_to_stopped_by_default_not_seeding() {
    let sm = StateMachine;
    assert!(sm.can_transition(&TaskState::Completed, &TaskState::Stopped, &ctx()));
    // 默认 Seeding 关闭 → Completed 不直接进 Seeding
    assert!(!sm.can_transition(&TaskState::Completed, &TaskState::Seeding, &ctx()));
    let mut seeding = ctx();
    seeding.seeding_enabled = true;
    assert!(sm.can_transition(&TaskState::Completed, &TaskState::Seeding, &seeding));
}

#[test]
fn any_to_failed_allowed() {
    let sm = StateMachine;
    for from in [
        TaskState::Queued,
        TaskState::Evaluating(EvalPhase::MetadataPending),
        TaskState::Downloading(EngineKind::Bt),
        TaskState::Transferring,
        TaskState::FallbackProvider,
    ] {
        assert!(
            sm.can_transition(&from, &TaskState::Failed, &ctx()),
            "{from:?}"
        );
    }
}

#[test]
fn illegal_transitions_rejected() {
    let sm = StateMachine;
    assert!(!sm.can_transition(&TaskState::Queued, &TaskState::Completed, &ctx()));
    assert!(!sm.can_transition(
        &TaskState::Queued,
        &TaskState::Downloading(EngineKind::Bt),
        &ctx()
    ));
    assert!(!sm.can_transition(
        &TaskState::Completed,
        &TaskState::Downloading(EngineKind::Bt),
        &ctx()
    ));
    assert!(!sm.can_transition(&TaskState::Paused, &TaskState::FallbackProvider, &ctx()));
}

#[test]
fn transition_returns_new_state_or_forbidden() {
    let sm = StateMachine;
    assert!(sm
        .transition(
            &TaskState::Queued,
            &TaskState::Evaluating(EvalPhase::MetadataPending),
            &ctx()
        )
        .is_ok());
    assert!(sm
        .transition(&TaskState::Queued, &TaskState::Completed, &ctx())
        .is_err());
}

#[test]
fn paused_to_downloading_via_resume() {
    let sm = StateMachine;
    assert!(sm.can_transition(
        &TaskState::Paused,
        &TaskState::Downloading(EngineKind::Bt),
        &ctx()
    ));
}

#[test]
fn default_policy_is_frozen() {
    // 设计文档 §9 默认值（D23）
    let p = FallbackPolicy::default();
    assert_eq!(p.bt_ratio_to_continue, 0.5);
    assert!(!p.allow_parallel_disk);
    assert_eq!(p.max_provider_redownloads, 2);
}
