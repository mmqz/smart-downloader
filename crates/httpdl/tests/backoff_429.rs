//! M4a: 重试退避（§14 D25：429/5xx 退避 1/2/4/8s×4）。不真等，纯函数断言延迟序列。

use smart_dl_httpdl::retry::{should_retry, Backoff};
use std::time::Duration;

#[test]
fn delay_sequence_is_1_2_4_8() {
    let b = Backoff::default(); // base 1s, max 8s
    assert_eq!(b.next_delay(1), Duration::from_secs(1));
    assert_eq!(b.next_delay(2), Duration::from_secs(2));
    assert_eq!(b.next_delay(3), Duration::from_secs(4));
    assert_eq!(b.next_delay(4), Duration::from_secs(8));
}

#[test]
fn delay_capped_at_max() {
    let b = Backoff::default();
    assert_eq!(b.next_delay(10), Duration::from_secs(8), "指数退避封顶 8s");
}

#[test]
fn retryable_statuses_are_429_and_5xx() {
    for code in [408, 429, 500, 502, 503, 504] {
        assert!(should_retry(code, 1, 4), "{code} 应可重试");
    }
}

#[test]
fn terminal_statuses_are_not_retryable() {
    for code in [200, 400, 401, 403, 404, 416] {
        assert!(!should_retry(code, 1, 4), "{code} 不应重试");
    }
}

#[test]
fn attempt_limit_stops_retry() {
    // 429×4 用尽（D25 ×4）→ 不再重试
    assert!(!should_retry(429, 4, 4));
    assert!(!should_retry(429, 5, 4));
}

#[test]
fn custom_backoff_respects_base_and_max() {
    let b = Backoff {
        base: Duration::from_millis(500),
        max: Duration::from_secs(2),
    };
    assert_eq!(b.next_delay(1), Duration::from_millis(500));
    assert_eq!(b.next_delay(3), Duration::from_secs(2), "2^2*0.5s=2s 达 max");
}