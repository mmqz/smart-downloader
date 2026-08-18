//! M7: WS 升级端点（D36 socket 端点）——连接即推全量、增量轮询、1s 快照节流
//! （Progress/Speed 合并）、关键事件直通、close 优雅退出。

use futures::StreamExt;
use smart_dl_daemon::events::{Envelope, SchedulerEvent};
use smart_dl_daemon::http;
use smart_dl_daemon::state::DaemonState;
use smart_dl_httpdl::HttpEngine;
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::{connect_async, MaybeTlsStream};

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

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

async fn connect(addr: &std::net::SocketAddr) -> Ws {
    let (ws, _resp) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("ws 升级必须成功");
    ws
}

/// 在窗口内收集全部事件帧（反序列化 Envelope）。
async fn collect_for(ws: &mut Ws, window: Duration) -> Vec<Envelope> {
    let mut out = Vec::new();
    loop {
        match tokio::time::timeout(window, ws.next()).await {
            Ok(Some(Ok(WsMessage::Text(t)))) => {
                if let Ok(env) = serde_json::from_str::<Envelope>(t.as_ref()) {
                    out.push(env);
                }
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_elapsed) => break,
        }
    }
    out
}

#[tokio::test]
async fn connect_pushes_immediate_events() {
    let (addr, state) = serve().await;
    let mut ws = connect(&addr).await;

    // 连接后发布：非节流事件应立即推（seq 严格递增）
    state.hub().publish(SchedulerEvent::TaskCreated {
        task_id: "t1".into(),
    });
    state.hub().publish(SchedulerEvent::StateChanged {
        task_id: "t1".into(),
        from: smart_dl_core::state_machine::TaskState::Queued,
        to: smart_dl_core::state_machine::TaskState::Downloading(
            smart_dl_core::types::EngineKind::Http,
        ),
    });

    let got = collect_for(&mut ws, Duration::from_secs(2)).await;
    assert_eq!(got.len(), 2, "两条事件应立即推送: {got:?}");
    assert_eq!(got[0].seq, 1);
    assert_eq!(got[1].seq, 2);
    assert!(matches!(got[0].event, SchedulerEvent::TaskCreated { .. }));
    assert!(matches!(got[1].event, SchedulerEvent::StateChanged { .. }));
}

#[tokio::test]
async fn progress_throttled_to_one_per_second_latest_value() {
    let (addr, state) = serve().await;
    let mut ws = connect(&addr).await;

    // 快速连发 5 条 Progress（同一任务）：节流应合并为 1s 一条最新值
    for done in [10u64, 20, 30, 40, 50] {
        state.hub().publish(SchedulerEvent::Progress {
            task_id: "t1".into(),
            done,
            total: 100,
        });
    }

    // 0.5s 窗口：不应有 Progress 提前到达（合并中）
    let early = collect_for(&mut ws, Duration::from_millis(500)).await;
    assert!(
        early
            .iter()
            .all(|e| !matches!(e.event, SchedulerEvent::Progress { .. })),
        "0.5s 内不应收到 Progress: {early:?}"
    );

    // 1.8s 窗口：收到 1 条 Progress = 最后一次值
    let late = collect_for(&mut ws, Duration::from_millis(1800)).await;
    let progress: Vec<&Envelope> = late
        .iter()
        .filter(|e| matches!(e.event, SchedulerEvent::Progress { .. }))
        .collect();
    assert_eq!(progress.len(), 1, "1s 节流应只发 1 条: {late:?}");
    match &progress[0].event {
        SchedulerEvent::Progress { done, .. } => assert_eq!(*done, 50, "应合并为最新值"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn critical_event_not_throttled() {
    let (addr, state) = serve().await;
    let mut ws = connect(&addr).await;

    // 关键事件（Completed）绕过节流立即推
    state.hub().publish(SchedulerEvent::Completed {
        task_id: "t1".into(),
    });
    let got = collect_for(&mut ws, Duration::from_secs(2)).await;
    assert_eq!(got.len(), 1);
    assert!(matches!(got[0].event, SchedulerEvent::Completed { .. }));
}

#[tokio::test]
async fn drain_resync_on_reconnect() {
    let (addr, state) = serve().await;
    // 先产生事件，再连接 → 连接即推全量（重连重同步）
    state.hub().publish(SchedulerEvent::TaskCreated {
        task_id: "t1".into(),
    });
    state.hub().publish(SchedulerEvent::Failed {
        task_id: "t1".into(),
        reason: "boom".into(),
    });

    let mut ws = connect(&addr).await;
    let got = collect_for(&mut ws, Duration::from_secs(2)).await;
    assert_eq!(got.len(), 2, "重连应收到全量历史: {got:?}");
    assert_eq!(got[0].seq, 1);
    assert_eq!(got[1].seq, 2);
}
