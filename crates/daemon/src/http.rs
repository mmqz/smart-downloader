//! HTTP API（axum，M6）：任务 CRUD + 快照 + Provider 运行态 + WS 升级端点（M7）。

use crate::state::DaemonState;
use crate::ws::Throttler;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
pub struct AddTaskReq {
    pub url: String,
    #[serde(default)]
    pub dest: Option<String>,
}

/// 组装 API 路由。
pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/tasks", get(list_tasks).post(add_task))
        .route("/tasks/:id", get(task_snapshot))
        .route("/tasks/:id/pause", post(pause_task))
        .route("/tasks/:id/resume", post(resume_task))
        .route("/providers", get(providers))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

/// WS 升级端点（D36 socket 端点，M7）：连接即推全量（重连重同步），随后
/// 轮询增量 + 1s 快照节流（Progress/Speed 合并）。单消费者语义（D22 本机单
/// 用户）；多连接时后来的连接会 drain 走队列，属可接受偏差。
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<DaemonState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_session(socket, state))
}

async fn ws_session(mut socket: WebSocket, state: Arc<DaemonState>) {
    let hub = state.hub();

    // 1) 连接即推送全量（掉队客户端重连重同步入口）
    let mut last_seq = 0u64;
    for env in hub.drain() {
        if send_text(&mut socket, &env).await.is_none() {
            return;
        }
        last_seq = env.seq;
    }

    // 2) 增量轮询 + 1s 节流
    let mut throttler = Throttler::new();
    let mut last_flush = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | None => return,
                Some(Ok(Message::Ping(p))) => {
                    if socket.send(Message::Pong(p)).await.is_err() { return; }
                }
                Some(Ok(_)) => {} // 本端点以推送为主，忽略上行文本
                Some(Err(_)) => return,
            },
            _ = tick.tick() => {
                for env in hub.snapshot_upto(last_seq) {
                    last_seq = env.seq;
                    if Throttler::is_throttlable(&env.event) {
                        throttler.upsert(env); // 合并，等 1s flush
                    } else if send_text(&mut socket, &env).await.is_none() {
                        return;
                    }
                }
                if !throttler.is_empty() && last_flush.elapsed() >= Duration::from_secs(1) {
                    for env in throttler.drain_pending() {
                        if send_text(&mut socket, &env).await.is_none() {
                            return;
                        }
                    }
                    last_flush = Instant::now();
                }
            }
        }
    }
}

/// 序列化 Envelope 为 WS 文本帧；失败（连接断开）返回 None。
async fn send_text(socket: &mut WebSocket, env: &crate::events::Envelope) -> Option<()> {
    let text = serde_json::to_string(env).ok()?;
    socket.send(Message::Text(text)).await.ok()
}

async fn add_task(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<AddTaskReq>,
) -> impl IntoResponse {
    match state.add_http_task(req.url, req.dest).await {
        Ok(task_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "task_id": task_id })),
        ),
        Err(e) => match e {
            crate::state::DaemonError::Duplicate(existing) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": format!("duplicate (existing: {existing})") })),
            ),
            other => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": other.to_string() })),
            ),
        },
    }
}

async fn task_snapshot(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.task_snapshot(&id).await {
        Some(snap) => Json(snap).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not found" })),
        )
            .into_response(),
    }
}

async fn list_tasks(State(state): State<Arc<DaemonState>>) -> impl IntoResponse {
    Json(state.list())
}

async fn pause_task(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.pause(&id).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn resume_task(
    State(state): State<Arc<DaemonState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.resume(&id).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn providers(State(state): State<Arc<DaemonState>>) -> impl IntoResponse {
    let rows: Vec<serde_json::Value> = state
        .provider_status()
        .iter()
        .map(|(name, rt)| {
            serde_json::json!({
                "provider": name,
                "enabled": rt.enabled,
                "authenticated": rt.authenticated,
                "quota_remaining": rt.quota_remaining,
                "busy": rt.busy,
                "backoff_until": rt.backoff_until,
                "last_error": rt.last_error,
            })
        })
        .collect();
    Json(rows)
}
