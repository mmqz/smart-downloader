//! M5: 配额/backoff/认证 —— quota=0 或 backoff 中 → Router 不选该 Provider；
//! quota 耗尽注入；并发上限。

mod common;

use smart_dl_core::ownership::FallbackPolicy;
use smart_dl_core::types::DownloadSource;
use smart_dl_provider::{FallbackCoordinator, MockProvider, ProviderError, RemoteProvider};
use std::sync::Arc;

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test]
async fn quota_zero_provider_not_selected() {
    let p = MockProvider::new("noquota").with_quota(0);
    let coord = FallbackCoordinator::new(vec![Arc::new(p)], FallbackPolicy::default());
    assert_eq!(coord.select_provider(), None, "quota=0 不得被选中");
}

#[tokio::test]
async fn backoff_provider_not_selected() {
    let p = MockProvider::new("backoff").with_backoff(now_unix() + 3600);
    let coord = FallbackCoordinator::new(vec![Arc::new(p)], FallbackPolicy::default());
    assert_eq!(coord.select_provider(), None, "backoff 中不得被选中");
}

#[tokio::test]
async fn unauthenticated_provider_not_selected() {
    let p = MockProvider::new("noauth").unauthenticated();
    let coord = FallbackCoordinator::new(vec![Arc::new(p)], FallbackPolicy::default());
    assert_eq!(coord.select_provider(), None, "未认证不得被选中");
}

#[tokio::test]
async fn disabled_provider_not_selected() {
    let p = MockProvider::new("off").disabled();
    let coord = FallbackCoordinator::new(vec![Arc::new(p)], FallbackPolicy::default());
    assert_eq!(coord.select_provider(), None, "disabled 不得被选中");
}

#[tokio::test]
async fn healthy_provider_is_selected() {
    let p = MockProvider::new("ok").with_quota(5);
    let coord = FallbackCoordinator::new(vec![Arc::new(p)], FallbackPolicy::default());
    assert_eq!(coord.select_provider().as_deref(), Some("ok"));
}

#[tokio::test]
async fn quota_exhausted_submit_fails() {
    let p = MockProvider::new("exhaust").with_quota(0);
    let r = p.submit(&DownloadSource::Magnet("m".into())).await;
    assert!(
        matches!(r, Err(ProviderError::Quota)),
        "quota=0 submit 必须失败"
    );
}

#[tokio::test]
async fn concurrency_full_blocks_selection() {
    let p = MockProvider::new("busy").with_concurrency(2);
    p.set_busy(2);
    let coord = FallbackCoordinator::new(vec![Arc::new(p)], FallbackPolicy::default());
    assert_eq!(
        coord.select_provider(),
        None,
        "并发满（busy==limit）不得被选中"
    );
}

#[tokio::test]
async fn refresh_auth_restores_authentication() {
    let p = MockProvider::new("reauth").unauthenticated();
    let coord = FallbackCoordinator::new(vec![Arc::new(p)], FallbackPolicy::default());
    assert_eq!(coord.select_provider(), None);
    coord.refresh_auth_all().await;
    assert_eq!(
        coord.select_provider().as_deref(),
        Some("reauth"),
        "refresh_auth 后可选"
    );
}

#[tokio::test]
async fn runtime_default_and_validity_helper() {
    // ProviderRuntime::default 构造 + files_all_valid 语义（覆盖类型层缺测分支）
    let rt: smart_dl_provider::ProviderRuntime = Default::default();
    assert!(rt.enabled && rt.authenticated);
    assert_eq!(rt.concurrency_limit, 2);
    let now = now_unix();
    let good = smart_dl_provider::ResolvedRemoteFile {
        rel_path: "a".into(),
        url: "http://x/a".into(),
        size: 1,
        etag: None,
        expires_at: None,
    };
    let expired_file = smart_dl_provider::ResolvedRemoteFile {
        expires_at: Some(now - 10),
        ..good.clone()
    };
    assert!(smart_dl_provider::coordinator::files_all_valid(
        &[good],
        now
    ));
    assert!(!smart_dl_provider::coordinator::files_all_valid(
        &[expired_file],
        now
    ));
}
