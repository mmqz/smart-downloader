//! M2: 任务去重（§4 去重 + D34 token 规则）。
//! 同一 btih 重复 → DuplicateRejected；带 token 无 validator 不认重；
//! 有 size validator 一致才认重。

use smart_dl_core::dedup::{DedupIndex, DedupOutcome};
use smart_dl_core::identity::{CanonicalId, CanonicalKind, Validator};

fn btih_canonical(ih: &str) -> CanonicalId {
    CanonicalId {
        kind: CanonicalKind::Bt,
        identity: ih.to_string(),
        validator: None,
        token_sensitive: false,
    }
}

#[test]
fn duplicate_btih_rejected() {
    let mut idx = DedupIndex::new();
    let c = btih_canonical("0123456789abcdef0123");
    assert_eq!(idx.check(&c, "t1".into()), DedupOutcome::Accepted);
    assert_eq!(idx.check(&c, "t2".into()), DedupOutcome::DuplicateRejected);
}

#[test]
fn distinct_btih_accepted() {
    let mut idx = DedupIndex::new();
    assert_eq!(idx.check(&btih_canonical("aaa"), "t1".into()), DedupOutcome::Accepted);
    assert_eq!(idx.check(&btih_canonical("bbb"), "t2".into()), DedupOutcome::Accepted);
}

#[test]
fn remove_frees_slot() {
    let mut idx = DedupIndex::new();
    let c = btih_canonical("xxx");
    idx.check(&c, "t1".into());
    assert_eq!(idx.remove(&c), Some("t1".into()));
    assert_eq!(idx.check(&c, "t2".into()), DedupOutcome::Accepted);
}

#[test]
fn token_url_without_validator_not_deduped() {
    // 带 token 且无 validator：即便归一化后 identity 相同也不自动认重（D34）
    let mut idx = DedupIndex::new();
    let c = CanonicalId {
        kind: CanonicalKind::Http,
        identity: "https://cdn.example.com/f.bin".into(),
        validator: None,
        token_sensitive: true,
    };
    assert_eq!(idx.check(&c, "t1".into()), DedupOutcome::Accepted);
    assert_eq!(idx.check(&c, "t2".into()), DedupOutcome::Accepted, "无 validator 不认重");
}

#[test]
fn token_url_with_matching_size_validator_deduped() {
    let mut idx = DedupIndex::new();
    let c1 = CanonicalId {
        kind: CanonicalKind::Http,
        identity: "https://cdn.example.com/f.bin".into(),
        validator: Some(Validator::Size(1000)),
        token_sensitive: true,
    };
    let c2 = c1.clone();
    assert_eq!(idx.check(&c1, "t1".into()), DedupOutcome::Accepted);
    assert_eq!(idx.check(&c2, "t2".into()), DedupOutcome::DuplicateRejected);
}

#[test]
fn token_url_with_differing_size_validator_not_deduped() {
    let mut idx = DedupIndex::new();
    let c1 = CanonicalId {
        kind: CanonicalKind::Http,
        identity: "https://cdn.example.com/f.bin".into(),
        validator: Some(Validator::Size(1000)),
        token_sensitive: true,
    };
    let c2 = CanonicalId {
        kind: CanonicalKind::Http,
        identity: "https://cdn.example.com/f.bin".into(),
        validator: Some(Validator::Size(2000)),
        token_sensitive: true,
    };
    assert_eq!(idx.check(&c1, "t1".into()), DedupOutcome::Accepted);
    assert_eq!(idx.check(&c2, "t2".into()), DedupOutcome::Accepted);
}

#[test]
fn plain_http_duplicate_rejected() {
    let mut idx = DedupIndex::new();
    let c = CanonicalId {
        kind: CanonicalKind::Http,
        identity: "https://a.com/f".into(),
        validator: Some(Validator::Size(42)),
        token_sensitive: false,
    };
    assert_eq!(idx.check(&c, "t1".into()), DedupOutcome::Accepted);
    assert_eq!(idx.check(&c, "t2".into()), DedupOutcome::DuplicateRejected);
}