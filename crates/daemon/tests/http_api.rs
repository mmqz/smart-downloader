//! M6: HTTP API（axum）——POST /tasks 添加（重复 → 409 + DuplicateRejected 事件）、
//! GET /tasks/:id 快照（跳号补拉入口）、GET /tasks 列表、pause/resume、
//! GET /providers 运行态快照。WS 升级端点为骨架（协议逻辑在 WsHub 测试覆盖）。

mod common;

use base64::Engine;
use common::{patterned, TestServer};
use smart_dl_daemon::events::SchedulerEvent;
use smart_dl_daemon::http;
use smart_dl_daemon::state::DaemonState;
use smart_dl_daemon::ws::WsHub;
use smart_dl_httpdl::HttpEngine;
use std::sync::Arc;

async fn serve() -> (std::net::SocketAddr, Arc<DaemonState>) {
    let engine = HttpEngine::new(reqwest::Client::new());
    let state = Arc::new(DaemonState::new(Arc::new(engine), vec![]));
    let app = http::router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, state)
}

/// 添加任务（dest 指向独立临时目录，避免引擎把产物写到测试 CWD）。
async fn add_task(
    client: &reqwest::Client,
    base: &str,
    url: &str,
) -> (reqwest::StatusCode, serde_json::Value) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dest = std::env::temp_dir().join(format!("m6-test-{nanos}-{}", rand_suffix()));
    let resp = client
        .post(format!("{base}/tasks"))
        .json(&serde_json::json!({ "url": url, "dest": dest.to_str().unwrap() }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.json().await.unwrap_or(serde_json::Value::Null);
    (status, body)
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
        ^ std::process::id() as u64
}

#[tokio::test]
async fn add_task_then_get_snapshot_and_list() {
    let body = patterned(64 * 1024);
    let srv = TestServer::start(body).await;
    let (addr, _state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let resp = add_task(&client, &base, &srv.url()).await;
    assert_eq!(resp.0, reqwest::StatusCode::CREATED, "添加任务必须 201");
    let created = resp.1;
    let tid = created["task_id"].as_str().unwrap().to_string();

    // GET /tasks/:id 快照（跳号补拉入口）
    let snap: serde_json::Value = client
        .get(format!("{base}/tasks/{tid}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snap["task_id"], tid);

    // GET /tasks 列表
    let list: serde_json::Value = client
        .get(format!("{base}/tasks"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn duplicate_add_rejected_with_event() {
    let body = patterned(32 * 1024);
    let srv = TestServer::start(body).await;
    let (addr, state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let first = add_task(&client, &base, &srv.url()).await;
    assert_eq!(first.0, reqwest::StatusCode::CREATED);

    let second = add_task(&client, &base, &srv.url()).await;
    assert_eq!(
        second.0,
        reqwest::StatusCode::CONFLICT,
        "重复 canonical 必须拒绝"
    );

    // DuplicateRejected 事件已发布（seq 递增）
    let drained = state.hub().drain();
    let events: Vec<&SchedulerEvent> = drained.iter().map(|e| &e.event).collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SchedulerEvent::DuplicateRejected { .. })),
        "重复拒绝必须发 DuplicateRejected 事件"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, SchedulerEvent::TaskCreated { .. })));
}

#[tokio::test]
async fn pause_resume_via_http() {
    let body = patterned(16 * 1024);
    let srv = TestServer::start(body).await;
    let (addr, state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let resp = add_task(&client, &base, &srv.url()).await;
    assert_eq!(resp.0, reqwest::StatusCode::CREATED);
    let tid = resp.1["task_id"].as_str().unwrap().to_string();

    let p = client
        .post(format!("{base}/tasks/{tid}/pause"))
        .send()
        .await
        .unwrap();
    assert!(p.status().is_success());
    let r = client
        .post(format!("{base}/tasks/{tid}/resume"))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    assert!(state.task_snapshot(&tid).await.is_some(), "任务仍存在");
}

#[tokio::test]
async fn provider_status_snapshot() {
    // Provider 健康/配额/冷却快照（GET /providers）
    let (addr, _state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(format!("{base}/providers"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(resp.is_array(), "providers 快照必须是数组");
}

#[tokio::test]
async fn unknown_task_returns_404() {
    let (addr, _state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/tasks/ghost"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[test]
fn hub_wired_into_state() {
    // DaemonState 持有 WsHub（事件发布统一入口）
    let engine = HttpEngine::new(reqwest::Client::new());
    let state = DaemonState::new(Arc::new(engine), vec![]);
    let hub: &WsHub = state.hub();
    assert_eq!(hub.last_seq(), 0);
}

#[tokio::test]
async fn same_resource_different_tokens_deduped_d34() {
    // D34：canonical 身份剥离 token 参数 → 同资源不同签名 token 判为同一任务（409）
    let body = patterned(16 * 1024);
    let srv = TestServer::start(body).await;
    let (addr, _state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let (s1, _) = add_task(&client, &base, &format!("{}?token=aaa", srv.url())).await;
    assert_eq!(s1, reqwest::StatusCode::CREATED, "首次添加应 201");

    let (s2, b2) = add_task(&client, &base, &format!("{}?token=bbb", srv.url())).await;
    assert_eq!(s2, reqwest::StatusCode::CONFLICT, "token 不同仍应判重复");
    assert!(
        b2["error"].as_str().unwrap().contains("duplicate"),
        "错误信息应含 duplicate: {b2}"
    );
}

#[tokio::test]
async fn distinct_query_params_are_distinct_tasks() {
    // 非 token 参数差异 → 不同 canonical → 允许添加
    let body = patterned(16 * 1024);
    let srv = TestServer::start(body).await;
    let (addr, _state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let (s1, _) = add_task(&client, &base, &format!("{}?v=1", srv.url())).await;
    assert_eq!(s1, reqwest::StatusCode::CREATED);
    let (s2, _) = add_task(&client, &base, &format!("{}?v=2", srv.url())).await;
    assert_eq!(s2, reqwest::StatusCode::CREATED, "v=1/v=2 是不同资源");
}

// ---- 迅雷链接家族归一化（thunder:// / qqdl://）----

fn thunder_link(real: &str) -> String {
    let inner = format!("AA{real}ZZ");
    format!(
        "thunder://{}",
        base64::engine::general_purpose::STANDARD.encode(inner.as_bytes())
    )
}

fn qqdl_link(real: &str) -> String {
    format!(
        "qqdl://{}",
        base64::engine::general_purpose::STANDARD.encode(real.as_bytes())
    )
}

#[tokio::test]
async fn thunder_link_decoded_and_added() {
    // thunder:// = base64("AA"+url+"ZZ") → 归一化后走 HTTP 引擎 → 201
    let body = patterned(16 * 1024);
    let srv = TestServer::start(body).await;
    let (addr, state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let resp = add_task(&client, &base, &thunder_link(&srv.url())).await;
    assert_eq!(
        resp.0,
        reqwest::StatusCode::CREATED,
        "thunder:// 应解码并 201"
    );
    let tid = resp.1["task_id"].as_str().unwrap().to_string();

    // 快照 source 是解码后的真实 URL（非 thunder:// 壳）
    let snap: serde_json::Value = client
        .get(format!("{base}/tasks/{tid}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let src = snap["source"].as_str().unwrap();
    assert!(!src.starts_with("thunder://"), "必须已解码: {src}");
    assert!(src.contains(&srv.url()), "含真实 URL: {src}");
}

#[tokio::test]
async fn qqdl_link_decoded_and_added() {
    // qqdl:// = base64(url)（无 AA/ZZ 壳）→ 201
    let body = patterned(16 * 1024);
    let srv = TestServer::start(body).await;
    let (addr, _state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let resp = add_task(&client, &base, &qqdl_link(&srv.url())).await;
    assert_eq!(resp.0, reqwest::StatusCode::CREATED, "qqdl:// 应解码并 201");
}

#[tokio::test]
async fn magnet_ed2k_unknown_rejected_with_clear_error() {
    // 归一化分类：magnet→BT(v1 无)；ed2k→不支持；未知→无法识别
    let (addr, _state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let (s, b) = add_task(&client, &base, "magnet:?xt=urn:btih:abc").await;
    assert_eq!(s, reqwest::StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("magnet"), "{b}");

    let (s, b) = add_task(&client, &base, "ed2k://file|a|1|hash|").await;
    assert_eq!(s, reqwest::StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("ed2k"), "{b}");

    let (s, b) = add_task(&client, &base, "sqla://whatever").await;
    assert_eq!(s, reqwest::StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("无法识别"), "{b}");

    // 畸形 thunder://（坏 base64）同样 400
    let (s, b) = add_task(&client, &base, "thunder://!!!not-base64!!!").await;
    assert_eq!(s, reqwest::StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("thunder"), "{b}");
}
