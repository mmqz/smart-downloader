//! M4b: 多连接并行下载（用户指定用例：4 段并行下载 64MB → SHA256 与源一致）。

mod common;
mod integration;

use common::{make_http_task_to, wait_terminal};
use integration::http_server::{patterned, sha256_of, HttpServerConfig, HttpTestServer};
use smart_dl_core::types::{DownloadEngine, EngineState};
use smart_dl_httpdl::{static_split, HttpEngine};

const MB: u64 = 1024 * 1024;

#[tokio::test]
async fn four_segments_64mb_sha256_matches_source() {
    // 用户指定用例：4 段并行下载 64MB → 文件 SHA256 与源一致
    let size = 64 * MB;
    let src = patterned(size);
    let expected = sha256_of(&src);
    let srv = HttpTestServer::start(HttpServerConfig {
        size,
        range: true,
        patterned_content: true,
        ..Default::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(reqwest::Client::new());
    let task = make_http_task_to(
        "i1",
        &srv.url("/file"),
        dir.path().to_path_buf(),
        Some("out.bin"),
    );
    let tid = engine.add(&task).await.unwrap();

    let st = wait_terminal(&engine, &tid).await;
    assert_eq!(st.state, EngineState::Completed, "error: {:?}", st.error);

    let got = std::fs::read(dir.path().join("out.bin")).unwrap();
    assert_eq!(sha256_of(&got), expected, "4 段并行下载结果必须与源一致");
    assert!(
        !dir.path().join("out.bin.part").exists(),
        "完成后 .part 应落位删除"
    );
}

#[tokio::test]
async fn part_file_released_after_finalize() {
    let srv = HttpTestServer::start(HttpServerConfig {
        size: MB,
        patterned_content: true,
        ..Default::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(reqwest::Client::new());
    let task = make_http_task_to(
        "i2",
        &srv.url("/file"),
        dir.path().to_path_buf(),
        Some("m.bin"),
    );
    let tid = engine.add(&task).await.unwrap();
    wait_terminal(&engine, &tid).await;

    assert!(dir.path().join("m.bin").exists());
    assert!(!dir.path().join("m.bin.part").exists());
}

#[tokio::test]
async fn segment_requests_cover_file_without_overlap() {
    // 5MB（公式 2 段）：server 记录的各段 Range 起点必须覆盖 [0, total) 且不重叠
    let size = 5 * MB;
    let srv = HttpTestServer::start(HttpServerConfig {
        size,
        ..Default::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(reqwest::Client::new());
    let task = make_http_task_to(
        "i3",
        &srv.url("/file"),
        dir.path().to_path_buf(),
        Some("c.bin"),
    );
    let tid = engine.add(&task).await.unwrap();
    wait_terminal(&engine, &tid).await;

    let starts: std::collections::HashSet<u64> =
        srv.range_starts.lock().unwrap().iter().copied().collect();
    // probe(0-0) 与段1 同起点；段起点集合必须恰为 {0, size/2} → 覆盖且不重叠
    assert_eq!(
        starts,
        [0u64, size / 2].into_iter().collect(),
        "2 段起点必须覆盖全文件且不相交"
    );
    assert_eq!(
        std::fs::metadata(dir.path().join("c.bin")).unwrap().len(),
        size
    );
}

#[test]
fn split_n_makes_n_disjoint_segments() {
    let segs = static_split::split_n(64 * MB, 4);
    assert_eq!(segs.len(), 4);
    for w in segs.windows(2) {
        assert_eq!(w[0].end + 1, w[1].start);
    }
    assert_eq!(segs.first().unwrap().start, 0);
    assert_eq!(segs.last().unwrap().end, 64 * MB - 1);
}
