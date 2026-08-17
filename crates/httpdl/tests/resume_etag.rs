//! M4a: .part 续传决策（§14 ETag 策略）。
//! ETag 一致 → 续；不一致但服务器尊重 Range（206 试探）→ 续；200/416/Length 变化 → 重下。

mod integration;

use smart_dl_httpdl::range::probe_range;
use smart_dl_httpdl::resume::{decide_resume, ResumeDecision};

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn etag_match_continues_from_part_len() {
    let srv = integration::http_server::HttpTestServer::start(
        integration::http_server::HttpServerConfig {
            size: 1024,
            range: true,
            etag: Some("etag-1"),
            ..Default::default()
        },
    )
    .await;
    let probe = probe_range(&client(), &srv.url("/file"), &[])
        .await
        .unwrap();
    let d = decide_resume(100, Some("etag-1"), &probe);
    assert_eq!(d, ResumeDecision::ContinueFrom(100));
}

#[tokio::test]
async fn etag_mismatch_but_server_respects_range_continues() {
    // ETag 变了（服务器换源/重生成）但服务器仍尊重 Range → 试探 206 → 从偏移续
    let srv = integration::http_server::HttpTestServer::start(
        integration::http_server::HttpServerConfig {
            size: 1024,
            range: true,
            etag: Some("etag-v2"),
            ..Default::default()
        },
    )
    .await;
    let probe = probe_range(&client(), &srv.url("/file"), &[])
        .await
        .unwrap();
    let d = decide_resume(100, Some("etag-1"), &probe);
    assert_eq!(d, ResumeDecision::ContinueFrom(100));
}

#[tokio::test]
async fn server_ignores_range_restarts() {
    let srv = integration::http_server::HttpTestServer::start(
        integration::http_server::HttpServerConfig {
            size: 1024,
            range: false,
            ..Default::default()
        },
    )
    .await;
    let probe = probe_range(&client(), &srv.url("/file"), &[])
        .await
        .unwrap();
    // ETag 不一致 + 服务器忽略 Range → 无法续传 → 重下
    let d = decide_resume(100, Some("old-etag"), &probe);
    assert_eq!(d, ResumeDecision::Restart, "忽略 Range → 无法续传 → 重下");
}

#[tokio::test]
async fn server_416_restarts() {
    let srv = integration::http_server::HttpTestServer::start(
        integration::http_server::HttpServerConfig {
            size: 1024,
            always_416: true,
            ..Default::default()
        },
    )
    .await;
    let probe = probe_range(&client(), &srv.url("/file"), &[])
        .await
        .unwrap();
    // ETag 不一致 + 416 → 范围非法 → 重下
    let d = decide_resume(100, Some("old-etag"), &probe);
    assert_eq!(d, ResumeDecision::Restart, "416 → 范围非法 → 重下");
}

#[test]
fn part_longer_than_file_restarts() {
    let probe = smart_dl_httpdl::range::Probe {
        range_supported: true,
        etag: Some("etag-1".into()),
        total: Some(100),
    };
    let d = decide_resume(150, Some("etag-1"), &probe);
    assert_eq!(
        d,
        ResumeDecision::Restart,
        "part 比文件还长（Length 变化）→ 重下"
    );
}

#[test]
fn no_etag_no_range_restarts() {
    let probe = smart_dl_httpdl::range::Probe {
        range_supported: false,
        etag: None,
        total: Some(100),
    };
    let d = decide_resume(10, None, &probe);
    assert_eq!(d, ResumeDecision::Restart);
}
