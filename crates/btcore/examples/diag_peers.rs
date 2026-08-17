//! 临时诊断（M1 peers 空转问题排查）：连接状态 vs get_peer_info 分离观察。
//! 用法：同测试环境变量（LT_KERNEL_LIB_DIR / SEED_MAIN / PATH），cargo run --example diag_peers

#[path = "../../../tests/integration/seed/mod.rs"]
mod seed;

use std::time::{Duration, Instant};

use smart_dl_btcore::BtCore;

fn main() {
    let save = seed::TempDir::new().expect("tempdir");
    let c = BtCore::new(save.path(), "diag-peers").expect("session");
    c.set_alert_mask(0xFFFF).expect("mask");
    let seeder = seed::TestSeeder::start();
    let (ip, port) = seeder.addr();
    // 对照组 A：裸 TCP 直达 seeder 端口（判断 listener 是否真的可达）
    match std::net::TcpStream::connect((ip.clone(), port)) {
        Ok(_) => eprintln!("RAW TCP: OK — seeder listener 可达"),
        Err(e) => eprintln!("RAW TCP: FAILED — {}", e),
    }
    let ih = c.add_magnet(seeder.magnet(), &[]).expect("add_magnet");
    c.add_peer(&ih, &ip, port).expect("add_peer");
    eprintln!("ih={} seeder={}:{}", ih, ip, port);

    let dl = Instant::now() + Duration::from_secs(12);
    while Instant::now() < dl {
        let st = c.status(&ih).expect("status");
        let np = c.peers(&ih).expect("peers");
        let alerts = c.pop_alerts(32).expect("alerts");
        eprintln!(
            "state={} prog={:.2} peers_raw={} meta={} alerts={}",
            st.state, st.progress, np.len(), st.metadata_received, alerts.len()
        );
        for a in alerts.iter() {
            eprintln!("   alert {:?}: {}", a.kind, a.msg);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    eprintln!("--- seeder log tail ---\n{}", seeder.log());
}