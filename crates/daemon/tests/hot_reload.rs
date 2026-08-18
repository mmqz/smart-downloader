//! #6 TOML 热重载：refresh_config 应用后可热更字段（default_dest_root + /config 快照）——
//! 改 dest_root → GET /config 立即反映 + 新任务（无 dest）落到新默认目录。

mod common;

use smart_dl_daemon::config::Config;
use smart_dl_daemon::http;
use smart_dl_daemon::state::DaemonState;
use smart_dl_httpdl::HttpEngine;
use std::path::PathBuf;
use std::sync::Arc;

fn cfg_with_dest(dest: PathBuf) -> Config {
    // 基于默认值改 dest_root（字段均 pub）
    let mut c = Config::default();
    c.download.dest_root = dest;
    c
}

#[tokio::test]
async fn refresh_config_updates_dest_root_and_snapshot() {
    let body = common::patterned(8 * 1024);
    let srv = common::TestServer::start(body).await;
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old");
    let new = dir.path().join("new");
    let tasks = dir.path().join("tasks.json");

    let engine = HttpEngine::new(reqwest::Client::new());
    let state = Arc::new(
        DaemonState::new(Arc::new(engine), vec![])
            .with_dest_root(old.clone())
            .with_config(Config::snapshot_json(&cfg_with_dest(old.clone()), &tasks)),
    );
    let app = http::router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // 初始快照：dest_root = old
    let snap0: serde_json::Value = client
        .get(format!("{base}/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snap0["dest_root"], old.to_string_lossy().as_ref());

    // #6 热重载：dest_root → new
    state.refresh_config(&cfg_with_dest(new.clone()), &tasks);

    // /config 立即反映
    let snap1: serde_json::Value = client
        .get(format!("{base}/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snap1["dest_root"], new.to_string_lossy().as_ref());

    // 新任务（无 dest）→ 落新默认目录
    let resp = client
        .post(format!("{base}/tasks"))
        .json(&serde_json::json!({ "url": srv.url() }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED, "add 应成功");
    let tid = resp.json::<serde_json::Value>().await.unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();
    let snap: serde_json::Value = client
        .get(format!("{base}/tasks/{tid}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        snap["dest_root"],
        new.to_string_lossy().as_ref(),
        "热重载后新任务必须落新 dest_root"
    );
}

#[tokio::test]
async fn no_reload_no_change() {
    // 不调用 refresh_config → dest_root 保持注入值（对照组）
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old");
    let tasks = dir.path().join("tasks.json");
    let engine = HttpEngine::new(reqwest::Client::new());
    let state = Arc::new(
        DaemonState::new(Arc::new(engine), vec![])
            .with_dest_root(old.clone())
            .with_config(Config::snapshot_json(&cfg_with_dest(old.clone()), &tasks)),
    );
    let app = http::router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let resp: serde_json::Value = reqwest::Client::new()
        .get(format!("http://{addr}/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp["dest_root"], old.to_string_lossy().as_ref());
}
