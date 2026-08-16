//! M0 E2E 验收测试（TDD 计划 M0 Step 1）：真实磁力 60s 内 progress>0。
//! 依赖：tests/integration/seed/ 本地 seeder 已起（2MB 测试文件，本地 tracker）。
//! 前置：M0 spike 冻结 D14 后，`Bare` 由 btcore 提供（手写 C ABI 或 cxx 实现，测试不关心）。
//! 当前为 TDD 红态：Bare 尚未实现，本测试编译失败即预期。

use std::time::{Duration, Instant};

use smart_dl_btcore::Bare;

#[test]
fn real_magnet_makes_progress_within_60s() {
    let seeder = TestSeeder::start(); // tests/integration/seed/ 提供本地 magnet
    let save = tempdir();
    let session = Bare::new(save.path().to_str().unwrap(), "m0").unwrap();

    let ih = session.add_magnet(&seeder.magnet(), &[]).unwrap();

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut progress = 0.0f32;
    loop {
        let (p, _state) = session.status(&ih).unwrap();
        progress = p;
        if p > 0.0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "progress stayed 0 for 60s (seeder={})",
            seeder.addr()
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(progress > 0.0, "progress must exceed 0");
}
