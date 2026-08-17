//! HTTP API（axum，M6）：任务 CRUD + 快照 + Provider 运行态 + WS 升级骨架。

use crate::state::DaemonState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

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
        .with_state(state)
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
