//! M4a: Range 探测（§14）。206 → 支持（多连接/续传能力）；200 → 单连接流式；416 → 重下。

mod integration;

use smart_dl_httpdl::range::probe_range;

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn server_206_reports_range_supported() {
    let srv = integration::http_server::HttpTestServer::start(
        integration::http_server::HttpServerConfig {
            size: 2048,
            range: true,
            etag: Some("etag-1"),
            ..Default::default()
        },
    )
    .await;
    let p = probe_range(&client(), &srv.url("/file"), &[])
        .await
        .unwrap();
    assert!(p.range_supported, "206 应报告支持 Range");
    assert_eq!(p.etag.as_deref(), Some("etag-1"));
    assert_eq!(p.total, Some(2048));
}

#[tokio::test]
async fn server_200_means_range_unsupported() {
    let srv = integration::http_server::HttpTestServer::start(
        integration::http_server::HttpServerConfig {
            size: 1024,
            range: false,
            ..Default::default()
        },
    )
    .await;
    let p = probe_range(&client(), &srv.url("/file"), &[])
        .await
        .unwrap();
    assert!(!p.range_supported, "忽略 Range（200）应报告不支持");
    assert_eq!(p.total, Some(1024));
}

#[tokio::test]
async fn server_416_means_range_unsupported() {
    let srv = integration::http_server::HttpTestServer::start(
        integration::http_server::HttpServerConfig {
            size: 1024,
            always_416: true,
            ..Default::default()
        },
    )
    .await;
    let p = probe_range(&client(), &srv.url("/file"), &[])
        .await
        .unwrap();
    assert!(!p.range_supported, "416 应报告不支持 Range");
}

#[tokio::test]
async fn probe_404_is_error_not_silent() {
    let srv = integration::http_server::HttpTestServer::start(Default::default()).await;
    let r = probe_range(&client(), &srv.url("/404"), &[]).await;
    assert!(r.is_err(), "404 应报错（文件级失败）");
}
