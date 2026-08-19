//! 引擎级限速集成：`HttpEngine::new_limited` 真实节流下载路径（跨段共享 RateLimiter），
//! 且不破坏完整性。冷启动语义：无积压时首个 chunk 立即放行（见 rate.rs 单测），
//! 故 2MiB @ 1MiB/s 的总耗时 ≈ 2s（节流对总量生效，而不是完全等速）。

mod common;
mod integration;

use common::{make_http_task_to, wait_terminal};
use integration::http_server::{patterned, sha256_of, HttpServerConfig, HttpTestServer};
use smart_dl_core::types::{DownloadEngine, EngineState};
use smart_dl_httpdl::HttpEngine;
use std::time::{Duration, Instant};

const MB: u64 = 1024 * 1024;

#[tokio::test]
async fn new_limited_throttles_download_path() {
    // 2MiB @ 1MiB/s：限速接线生效 → 总耗时 ≈ 2s；回归（未接线）→ 本机 mock <200ms。
    let srv = HttpTestServer::start(HttpServerConfig {
        size: 2 * MB,
        range: true,
        patterned_content: true,
        ..Default::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new_limited(reqwest::Client::new(), 1024); // 1MiB/s
    let task = make_http_task_to(
        "lim1",
        &srv.url("/file"),
        dir.path().to_path_buf(),
        Some("out.bin"),
    );
    let tid = engine.add(&task).await.unwrap();

    let t0 = Instant::now();
    let st = wait_terminal(&engine, &tid).await;
    let el = t0.elapsed();
    assert_eq!(st.state, EngineState::Completed, "error: {:?}", st.error);
    assert!(
        el >= Duration::from_millis(900),
        "限速未生效（总量 2MiB @ 1MiB/s 应 ≈2s，实际 {el:?}）"
    );
    assert!(el < Duration::from_secs(8), "异常超长: {el:?}");

    let got = std::fs::read(dir.path().join("out.bin")).unwrap();
    assert_eq!(
        sha256_of(&got),
        sha256_of(&patterned(2 * MB)),
        "限速不得破坏完整性"
    );
}

#[tokio::test]
async fn unlimited_is_fast_by_contrast() {
    // 对照组：不限速引擎下载同尺寸应远快于限速用例（区分"限速生效"与"环境慢"）
    let srv = HttpTestServer::start(HttpServerConfig {
        size: 2 * MB,
        range: true,
        patterned_content: true,
        ..Default::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(reqwest::Client::new());
    let task = make_http_task_to(
        "lim2",
        &srv.url("/file"),
        dir.path().to_path_buf(),
        Some("out.bin"),
    );
    let tid = engine.add(&task).await.unwrap();

    let t0 = Instant::now();
    let st = wait_terminal(&engine, &tid).await;
    let el = t0.elapsed();
    assert_eq!(st.state, EngineState::Completed, "error: {:?}", st.error);
    assert!(
        el < Duration::from_millis(900),
        "不限速引擎 2MiB 本机下载应 <900ms（实际 {el:?}）；若环境过慢需放宽限速用例阈值"
    );
}
