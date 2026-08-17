//! M4 测试基建：可配置 HTTP server（axum）。
//! 行为：支持/忽略 Range（206/200）、416、ETag、前 N 次 429；记录每次 Range 请求起点。

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Clone)]
pub struct HttpServerConfig {
    /// 文件总大小（字节）。
    pub size: u64,
    /// 尊重 Range（206）；false → 忽略（总是 200 全文件）。
    pub range: bool,
    /// 所有 Range 请求回 416（+Content-Range: bytes */size）。
    pub always_416: bool,
    pub etag: Option<&'static str>,
    /// 前 N 次请求回 429（按请求计数）。
    pub retry_429: u32,
}

impl Default for HttpServerConfig {
    fn default() -> Self {
        HttpServerConfig {
            size: 1024,
            range: true,
            always_416: false,
            etag: Some("etag-1"),
            retry_429: 0,
        }
    }
}

/// 测试基建：可配置 HTTP server。range_starts/request_count 供 M4b 断言（当前骨架未用）。
#[allow(dead_code)]
pub struct HttpTestServer {
    pub addr: SocketAddr,
    /// 每次带 Range 的请求的起点（验证续传位置）。
    pub range_starts: Arc<Mutex<Vec<u64>>>,
    pub request_count: Arc<AtomicUsize>,
}

impl HttpTestServer {
    pub async fn start(cfg: HttpServerConfig) -> Self {
        let range_starts = Arc::new(Mutex::new(Vec::new()));
        let request_count = Arc::new(AtomicUsize::new(0));
        let body = vec![0x5Au8; cfg.size as usize];

        let app = Router::new()
            .route("/file", get(handler))
            .route("/404", get(|| async { StatusCode::NOT_FOUND }))
            .with_state(ServerState {
                cfg,
                body,
                range_starts: range_starts.clone(),
                request_count: request_count.clone(),
            });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        HttpTestServer {
            addr,
            range_starts,
            request_count,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

#[derive(Clone)]
struct ServerState {
    cfg: HttpServerConfig,
    body: Vec<u8>,
    range_starts: Arc<Mutex<Vec<u64>>>,
    request_count: Arc<AtomicUsize>,
}

async fn handler(State(st): State<ServerState>, headers: HeaderMap) -> Response {
    let req_no = st.request_count.fetch_add(1, Ordering::SeqCst);
    if (req_no as u32) < st.cfg.retry_429 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }

    let mut builder = Response::builder().header(header::CONTENT_LENGTH, st.body.len().to_string());
    if let Some(etag) = st.cfg.etag {
        builder = builder.header(header::ETAG, etag);
    }

    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_string);
    match range {
        Some(r) if !st.cfg.always_416 => {
            let start = parse_range_start(&r);
            st.range_starts.lock().unwrap().push(start);
            if st.cfg.range {
                let total = st.body.len() as u64;
                let cr = format!("bytes {}-{}/{}", start, total - 1, total);
                let body = st.body.get(start as usize..).unwrap_or(&[]).to_vec();
                builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_RANGE, cr)
                    .body(axum::body::Body::from(body))
                    .unwrap()
                    .into_response()
            } else {
                // 忽略 Range：200 全文件
                builder
                    .status(StatusCode::OK)
                    .body(axum::body::Body::from(st.body.clone()))
                    .unwrap()
                    .into_response()
            }
        }
        Some(_) => {
            // 416：Content-Range: bytes */total
            builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{}", st.body.len()))
                .body(axum::body::Body::empty())
                .unwrap()
                .into_response()
        }
        None => builder
            .status(StatusCode::OK)
            .body(axum::body::Body::from(st.body.clone()))
            .unwrap()
            .into_response(),
    }
}

/// 解析 "bytes=START-END" 的起点。
fn parse_range_start(range: &str) -> u64 {
    range
        .strip_prefix("bytes=")
        .and_then(|r| r.split('-').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}