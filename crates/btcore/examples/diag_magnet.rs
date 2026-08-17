//! 诊断工具（M0 调试用，非测试）：真实 seeder 直连注入后打印逐秒完整状态 + alert 流。

#[path = "../../../tests/integration/seed/mod.rs"]
mod seed;

use std::time::{Duration, Instant};

fn main() {
    let seeder = seed::TestSeeder::start();
    let save = seed::TempDir::new().expect("tempdir");
    let session = smart_dl_btcore::Bare::new(save.path(), "diag").expect("session");

    let ih = session
        .add_magnet(seeder.magnet(), &[])
        .expect("add_magnet");
    println!("magnet: {}", seeder.magnet());
    println!("ih:     {}", ih);
    let (ip, port) = seeder.addr();
    println!("seeder: {}:{}", ip, port);
    session.add_peer(&ih, &ip, port).expect("add_peer");
    session.diag_set_mask(0xFFFF).expect("set_mask");

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut i = 0;
    loop {
        i += 1;
        let (p, state) = session.status(&ih).expect("status");
        let (meta, peers, seeds) = session.status_extra(&ih).expect("status_extra");
        let alerts = session.diag_pop_alerts().expect("pop");
        println!(
            "[{:2}s] progress={:.4} state={} meta={} peers={} seeds={} alerts={}",
            i,
            p,
            state,
            if meta > 0 { "Y" } else { "n" },
            peers,
            seeds,
            alerts.len()
        );
        for a in alerts.iter().take(6) {
            println!("        {}", a);
        }
        if p > 0.0 || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }

    // 打印 seeder 侧诊断（状态/告警行）
    drop(session);
    println!("\n===== seeder log (tail) =====");
    let log = seeder.log();
    for line in log.lines().skip(log.lines().count().saturating_sub(40)) {
        println!("{}", line);
    }
}
