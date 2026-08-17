//! M6: HTTP API（axum）——POST /tasks 添加（重复 → 409 + DuplicateRejected 事件）、
//! GET /tasks/:id 快照（跳号补拉入口）、GET /tasks 列表、pause/resume、
//! GET /providers 运行态快照。WS 升级端点为骨架（协议逻辑在 WsHub 测试覆盖）。

mod common;

use common::{patterned, TestServer};
use smart_dl_daemon::events::SchedulerEvent;
use smart_dl_daemon::http;
use smart_dl_daemon::state::DaemonState;
use smart_dl_daemon::ws::WsHub;
use smart_dl_httpdl::HttpEngine;
use std::sync::Arc;

async fn serve() -> (std::net::SocketAddr, Arc<DaemonState>) {
    let engine = HttpEngine::new(reqwest::Client::new());
    let state = Arc::new(DaemonState::new(engine, vec![]));
    let app = http::router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, state)
}

#[tokio::test]
async fn add_task_then_get_snapshot_and_list() {
    let body = patterned(64 * 1024);
    let srv = TestServer::start(body).await;
    let (addr, _state) = serve().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/tasks"))
        .json(&serde_json::json!({ "url": srv.url() }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "添加任务必须 201");
    let created: serde_json::Value = resp.json().await.unwrap();
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

    let first = client
        .post(format!("{base}/tasks"))
        .json(&serde_json::json!({ "url": srv.url() }))
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 201);

    let second = client
        .post(format!("{base}/tasks"))
        .json(&serde_json::json!({ "url": srv.url() }))
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), 409, "重复 canonical 必须拒绝");

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

    let resp = client
        .post(format!("{base}/tasks"))
        .json(&serde_json::json!({ "url": srv.url() }))
        .send()
        .await
        .unwrap();
    let tid = resp.json::<serde_json::Value>().await.unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

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
    let state = DaemonState::new(engine, vec![]);
    let hub: &WsHub = state.hub();
    assert_eq!(hub.last_seq(), 0);
}
