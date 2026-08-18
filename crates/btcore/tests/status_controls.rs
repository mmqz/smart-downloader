//! M1 状态/进度/控制测试：status 单调、piece_count/bitfield、file_progress、
//! limits/tracker/sequential/read_piece、暂停-恢复-移除流、无 seeder 元数据状态。

#[path = "../../../tests/integration/seed/mod.rs"]
mod seed;

use std::time::{Duration, Instant};

use smart_dl_btcore::BtCore;

const FILE_SIZE: i64 = 2 * 1024 * 1024; // seed_main 确定性 2MB
const PIECE_LEN: i64 = 16384;
const PIECES: usize = (FILE_SIZE / PIECE_LEN) as usize; // 128

fn core(tag: &str) -> (BtCore, seed::TempDir) {
    let save = seed::TempDir::new().expect("tempdir");
    let c = BtCore::new(save.path(), tag).expect("session");
    (c, save)
}

fn download_to_complete(c: &BtCore, ih: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let st = c.status(ih).expect("status");
        if st.progress >= 1.0 && st.state == 1 {
            return;
        }
        assert!(Instant::now() < deadline, "did not finish in 60s");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn status_metrics_and_geometry() {
    let (c, _save) = core("geo");
    let seeder = seed::TestSeeder::start();
    let ih = c.add_magnet(seeder.magnet(), &[]).expect("add_magnet");
    let (ip, port) = seeder.addr();

    // 无 seeder 连接前：元数据未获取 → state 4
    let st0 = c.status(&ih).expect("status0");
    assert_eq!(st0.state, 4, "未连 peers 时应为 downloading_metadata(4)");
    assert!(!st0.metadata_received);
    assert_eq!(c.piece_count(&ih).expect("piece0"), 0);
    assert!(
        c.bitfield(&ih).expect("bitfield0").is_empty(),
        "元数据前 bitfield 空"
    );

    c.add_peer(&ih, &ip, port).expect("add_peer");
    // 下载中：metadata 已收，peers ≥1，progress 单调上升
    let dl = Instant::now() + Duration::from_secs(30);
    let mut last = 0.0f32;
    loop {
        let st = c.status(&ih).expect("status");
        assert!(
            st.metadata_received || st.progress == 0.0,
            "metadata 后才能有进度"
        );
        assert!(
            st.progress >= last - 1e-6,
            "progress 应单调: {} -> {}",
            last,
            st.progress
        );
        last = st.progress;
        assert!((0.0..=1.0).contains(&st.progress));
        assert!(st.downloaded <= st.total);
        if st.progress >= 1.0 && st.state == 1 {
            break;
        }
        if st.progress > 0.0 {
            assert!(st.num_peers >= 1, "有进度时应有已连 peer");
        }
        assert!(Instant::now() < dl, "30s 未完成");
        std::thread::sleep(Duration::from_millis(200));
    }

    // 形状：128 pieces；bitfield 16 字节全 1
    assert_eq!(c.piece_count(&ih).expect("pieces"), PIECES as i32);
    let bf = c.bitfield(&ih).expect("bitfield");
    assert_eq!(bf.len(), PIECES.div_ceil(8), "bitfield 字节数");
    assert!(
        bf.iter().all(|&b| b == 0xFF),
        "完成后 bitfield 全 1: {:?}",
        bf
    );

    // 文件：单文件 2MB；进度=大小
    assert_eq!(c.file_count(&ih).expect("files"), 1);
    let fp = c.file_progress(&ih).expect("file_progress");
    assert_eq!(fp.len(), 1);
    assert_eq!(fp[0].1, FILE_SIZE, "文件大小 2MB");
    assert_eq!(fp[0].0, FILE_SIZE, "完成后已下载=大小");
    let st = c.status(&ih).expect("status_final");
    assert_eq!(st.total, FILE_SIZE);
    assert_eq!(st.downloaded, FILE_SIZE);
}

#[test]
fn control_ops_and_read_piece() {
    let (c, _save) = core("ctrl");
    let seeder = seed::TestSeeder::start();
    let ih = c.add_magnet(seeder.magnet(), &[]).expect("add_magnet");
    let (ip, port) = seeder.addr();
    c.add_peer(&ih, &ip, port).expect("add_peer");
    download_to_complete(&c, &ih);

    // 控制面
    c.set_limits(&ih, 0, 0).expect("limits unrestricted");
    c.set_limits(&ih, 1 << 20, 1 << 20).expect("limits 1MB");
    c.set_sequential(&ih, true).expect("sequential on");
    c.add_tracker(&ih, "http://127.0.0.1:9/announce")
        .expect("add_tracker");
    c.add_url_seed(&ih, "http://127.0.0.1:9/seed")
        .expect("add_url_seed");

    // read_piece 轮询（v2）：完成后 piece 0 = 16384 B
    let dl = Instant::now() + Duration::from_secs(10);
    let mut data: Option<Vec<u8>> = None;
    while Instant::now() < dl {
        match c.read_piece(&ih, 0) {
            Ok(Some(d)) => {
                data = Some(d);
                break;
            }
            Ok(None) => {
                let _ = c.pop_alerts(64);
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => panic!("read_piece 错误: {}", e),
        }
    }
    let data = data.expect("10s 内 piece0 未就绪");
    assert_eq!(data.len(), PIECE_LEN as usize, "piece 0 长度");

    // 移除（不删数据）→ 状态 NotFound
    c.remove(&ih, false).expect("remove");
    match c.status(&ih) {
        Err(smart_dl_btcore::Error::NotFound(_)) => {}
        other => panic!("移除后 status 应 NotFound，实际 {:?}", other),
    }
}

#[test]
fn pause_resume_flow() {
    // pause → torrent_paused alert；resume 后状态可查（ABI100：状态停在暂停前值，§10.1）
    let (c, _save) = core("pr");
    c.set_alert_mask(0xFFFF).expect("mask");
    let seeder = seed::TestSeeder::start();
    let ih = c.add_magnet(seeder.magnet(), &[]).expect("add_magnet");
    let (ip, port) = seeder.addr();
    c.add_peer(&ih, &ip, port).expect("add_peer");
    download_to_complete(&c, &ih);

    c.pause(&ih).expect("pause");
    let dl = Instant::now() + Duration::from_secs(10);
    let mut paused = false;
    while Instant::now() < dl {
        for a in c.pop_alerts(256).expect("pop") {
            if a.kind == smart_dl_btcore::AlertKind::State
                && a.state_subkind() == smart_dl_btcore::StateSubKind::Paused
            {
                paused = true;
            }
        }
        if paused {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(paused, "pause 后应收到 torrent_paused alert");
    let st = c.status(&ih).expect("status_paused");
    assert_eq!(st.progress, 1.0, "暂停后 progress 保持 1.0");

    c.resume(&ih).expect("resume");
    let st = c.status(&ih).expect("status_resumed");
    assert_eq!(st.progress, 1.0);
}
