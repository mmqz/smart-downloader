//! M2: canonical URL 归一化与 token 参数黑名单（§7 + D34）。
//! ?token=/?X-Amz-Signature=/?expires=/?auth= 从 identity 剔除；
//! ?v=1 未命中黑名单 → 参与 identity。

use smart_dl_core::source_parse::canonical_token::{normalize_http_url, TOKEN_PARAM_BLACKLIST};

#[test]
fn token_param_stripped_from_identity() {
    let (identity, sensitive) = normalize_http_url("https://a.com/f?token=abc&x=1");
    assert_eq!(identity, "https://a.com/f?x=1");
    assert!(sensitive);
}

#[test]
fn aws_signature_stripped() {
    let (identity, sensitive) =
        normalize_http_url("https://a.com/f?X-Amz-Signature=xyz&X-Amz-Credential=c");
    assert_eq!(identity, "https://a.com/f");
    assert!(sensitive);
}

#[test]
fn expires_and_auth_stripped() {
    let (identity, sensitive) = normalize_http_url("https://a.com/f?expires=1700000000&auth=tok");
    assert_eq!(identity, "https://a.com/f");
    assert!(sensitive);
}

#[test]
fn google_and_tencent_and_qiniu_prefixes_stripped() {
    for (key, val) in [
        ("X-Goog-Signature", "g"),
        ("X-Tencent-Authorization", "t"),
        ("X-QiNiu-Token", "q"),
    ] {
        let (identity, sensitive) = normalize_http_url(&format!("https://a.com/f?{key}={val}&v=1"));
        assert!(!identity.contains(key), "{key} 应被剔除: {identity}");
        assert!(sensitive);
        assert!(identity.contains("v=1"), "非 token query 应保留");
    }
}

#[test]
fn v_param_not_in_blacklist_participates_in_identity() {
    let (i1, s1) = normalize_http_url("https://a.com/f?v=1");
    let (i2, s2) = normalize_http_url("https://a.com/f?v=2");
    assert_ne!(i1, i2, "?v=1 与 ?v=2 参与 identity → 不同");
    assert!(!s1 && !s2, "v 不是 token 参数");
}

#[test]
fn fragment_removed() {
    let (identity, _) = normalize_http_url("https://a.com/f#section");
    assert_eq!(identity, "https://a.com/f");
}

#[test]
fn plain_url_untouched() {
    let (identity, sensitive) = normalize_http_url("https://a.com/f.bin?size=3");
    assert_eq!(identity, "https://a.com/f.bin?size=3");
    assert!(!sensitive);
}

#[test]
fn blacklist_contains_known_tokens() {
    for t in ["token", "sig", "signature", "expires", "auth"] {
        assert!(TOKEN_PARAM_BLACKLIST.contains(&t), "{t} 应在黑名单");
    }
}