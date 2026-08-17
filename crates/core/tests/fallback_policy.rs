//! M2: 兜底策略（§9 FallbackPolicy + Q-B9 metadata 超时）。
//! BT 49% → 允许兜底（先暂停，串行）；51% → 拒绝自动兜底（仅手动）；
//! allow_parallel_disk=false → 先 pause 再 Provider；
//! metadata 60s 超时 → 不触发 Provider，置 FallbackAvailable 标志。

use smart_dl_core::ownership::{
    decide_auto_fallback, on_metadata_timeout, FallbackDecision, FallbackPolicy, MetadataAction,
};

#[test]
fn bt_49_percent_allows_fallback() {
    let policy = FallbackPolicy::default();
    let d = decide_auto_fallback(0.49, &policy);
    // 默认 allow_parallel_disk=false → 需要先暂停（串行兜底），但允许兜底
    assert!(matches!(d, FallbackDecision::RequiresPauseFirst));
}

#[test]
fn bt_49_percent_parallel_allowed_when_disk_ok() {
    let policy = FallbackPolicy {
        allow_parallel_disk: true,
        ..FallbackPolicy::default()
    };
    let d = decide_auto_fallback(0.49, &policy);
    assert!(matches!(d, FallbackDecision::Auto));
}

#[test]
fn bt_51_percent_rejects_auto_fallback() {
    let policy = FallbackPolicy::default();
    let d = decide_auto_fallback(0.51, &policy);
    assert!(matches!(d, FallbackDecision::ManualOnly));
}

#[test]
fn ratio_boundary_exactly_half_allows() {
    // <0.5 才自动兜底；恰好 0.5 不触发
    let policy = FallbackPolicy::default();
    assert!(matches!(decide_auto_fallback(0.499, &policy), FallbackDecision::RequiresPauseFirst));
    assert!(matches!(decide_auto_fallback(0.5, &policy), FallbackDecision::ManualOnly));
}

#[test]
fn metadata_timeout_never_triggers_provider() {
    // Q-B9 写死：metadata 60s 超时 → 保持 BT + FallbackAvailable 标志（手动兜底）
    let action = on_metadata_timeout();
    assert!(matches!(action, MetadataAction::KeepBt { fallback_available: true }));
}

#[test]
fn default_policy_values_frozen() {
    let p = FallbackPolicy::default();
    assert_eq!(p.bt_ratio_to_continue, 0.5);
    assert!(!p.allow_parallel_disk);
    assert_eq!(p.max_provider_redownloads, 2);
}