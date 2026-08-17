//! M6: 富 peer 反吸血（§11 + qBittorrent-EE 黑名单规则）——
//! peer_id -XL/XL/-SD/-BN/-DT 或 client 含 Xunlei/Thunder → LeechDetected；正常不触发。

use smart_dl_core::types::PeerInfo;
use smart_dl_daemon::health::{detect_leech, leech_reason, HealthEventKind};

fn peer(peer_id: &str, client: &str) -> PeerInfo {
    PeerInfo {
        ip: "1.2.3.4".into(),
        port: 6881,
        peer_id: peer_id.into(),
        client: client.into(),
        progress_ppm: 500_000,
        down_rate: 100,
        up_rate: 0,
        total_download: 1_000_000,
        total_upload: 0,
        last_active_sec: 5,
        flags: "-".into(),
    }
}

#[test]
fn xunlei_peer_id_prefix_detected() {
    // -XL0012-…（Xunlei 变体）→ LeechDetected
    let p = peer("-XL0012-cafebabe", "Mainline");
    assert_eq!(detect_leech(&p), Some(HealthEventKind::LeechDetected));
    let r = leech_reason(&p);
    assert!(r.is_some());
}

#[test]
fn xunlei_client_string_detected() {
    let p = peer("-qB4390-xxxxxx", "Xunlei 0.1.0");
    assert_eq!(detect_leech(&p), Some(HealthEventKind::LeechDetected));
}

#[test]
fn qbittorrent_peer_not_detected() {
    let p = peer("-qB4390-6cY9xMX3", "qBittorrent 4.3.9");
    assert_eq!(detect_leech(&p), None, "qB 正常 client 不触发");
}

#[test]
fn sd_bn_dt_prefixes_detected() {
    for pref in ["-SD", "-BN", "-DT"] {
        let p = peer(&format!("{pref}0001-abc"), "Mainline");
        assert_eq!(
            detect_leech(&p),
            Some(HealthEventKind::LeechDetected),
            "{pref} 前缀需检测"
        );
    }
}

#[test]
fn unknown_peer_not_detected() {
    let p = peer("-TR3000-abcdef", "Transmission 3.0.0");
    assert_eq!(detect_leech(&p), None);
}
