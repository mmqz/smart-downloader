//! M6: 上传/下载比（§11）——累计 ratio <0.5 → RatioLow（仅统计告警）；≥0.5 不触发。

use smart_dl_core::types::PeerInfo;
use smart_dl_daemon::health::{check_ratio, ratio_low, HealthEventKind};

#[test]
fn ratio_below_half_detected() {
    // 上传 40MB / 下载 100MB = 0.4 < 0.5 → RatioLow
    let low = check_ratio(40 * 1024 * 1024, 100 * 1024 * 1024);
    assert_eq!(low, Some(HealthEventKind::RatioLow));
    assert!(ratio_low(40, 100));
}

#[test]
fn ratio_at_half_not_detected() {
    assert_eq!(
        check_ratio(50 * 1024 * 1024, 100 * 1024 * 1024),
        None,
        "0.5 边界不触发"
    );
    assert!(!ratio_low(50, 100));
}

#[test]
fn ratio_above_half_not_detected() {
    assert_eq!(check_ratio(80 * 1024 * 1024, 100 * 1024 * 1024), None);
    assert!(!ratio_low(80, 100));
}

#[test]
fn zero_download_does_not_panic() {
    // 无下载量 → 不算告警（避免除零/空分队误报）
    assert!(!ratio_low(0, 0));
    assert_eq!(check_ratio(1_000_000, 0), None);
}

#[test]
fn aggregate_across_peers_uses_totals() {
    // 多 peer 累计：sum(upload)/sum(download)
    let peers = [
        PeerInfo {
            total_download: 60,
            total_upload: 5,
            ..Default::default()
        },
        PeerInfo {
            total_download: 40,
            total_upload: 5,
            ..Default::default()
        },
    ];
    let up: u64 = peers.iter().map(|p| p.total_upload).sum();
    let down: u64 = peers.iter().map(|p| p.total_download).sum();
    assert!(ratio_low(up, down), "累计 10/100=0.1 <0.5 → RatioLow");
}
