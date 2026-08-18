//! serve 组装：单实例锁 → 引擎（HTTP [+BT]）→ DaemonState → HTTP/WS + BT alert 事件流 → 优雅退出。

use crate::config::Config;
use crate::http;
use crate::lockfile::InstanceLock;
use crate::state::DaemonState;
use std::sync::Arc;
#[cfg(feature = "bt")]
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("单实例锁: {0}")]
    Lock(#[from] crate::lockfile::LockError),
    #[error("引擎初始化: {0}")]
    Engine(String),
    #[error("监听 {0}: {1}")]
    Bind(String, std::io::Error),
    #[error("serve: {0}")]
    Io(#[from] std::io::Error),
}

/// 运行 daemon（阻塞至 Ctrl+C / 服务错误）。
pub async fn run(cfg: Config) -> Result<(), ServeError> {
    // 1. 单实例锁（重复启动立即退出）
    let _lock = InstanceLock::acquire(&cfg.lock.path)?;
    tracing::info!("单实例锁已持有: {:?}", cfg.lock.path);

    // 2. dest_root 预检（缺失目录自动创建）
    crate::state::ensure_dest_root(Some(cfg.download.dest_root.to_string_lossy().into_owned()))
        .map_err(|e| ServeError::Engine(format!("dest_root 预检失败: {e}")))?;

    // 3. 引擎组装：HTTP（必需）+ BT（feature bt 且配置开启）
    let http_engine: Arc<dyn smart_dl_core::types::DownloadEngine> =
        Arc::new(smart_dl_httpdl::HttpEngine::new(reqwest::Client::new()));
    #[cfg(feature = "bt")]
    let mut state =
        DaemonState::new(http_engine, vec![]).with_dest_root(cfg.download.dest_root.clone());
    #[cfg(not(feature = "bt"))]
    let mut state =
        DaemonState::new(http_engine, vec![]).with_dest_root(cfg.download.dest_root.clone());

    // 4. BT 引擎（先取 core 句柄，供 alert 事件流）
    #[cfg(feature = "bt")]
    let bt_core = {
        if cfg.bt.enabled {
            let save = cfg.bt_save_path();
            std::fs::create_dir_all(&save)
                .map_err(|e| ServeError::Engine(format!("BT 落盘目录创建失败 {save:?}: {e}")))?;
            let bt = crate::bt::BtEngine::new(&save).map_err(ServeError::Engine)?;
            let core = bt.core(); // Arc<BtCore>：alert 轮询句柄（trait 化前保存）
            let bt_arc: Arc<dyn smart_dl_core::types::DownloadEngine> = Arc::new(bt);
            state = state.with_bt(bt_arc);
            tracing::info!("BT 引擎已启用, 落盘: {save:?}");
            Some(core)
        } else {
            None
        }
    };
    #[cfg(not(feature = "bt"))]
    if cfg.bt.enabled {
        tracing::warn!("配置启用了 bt 但编译未带 --features bt，BT 不可用");
    }

    // 4b. 任务持久化 + 启动恢复（须在引擎表就绪后：恢复会重新 add）
    let tasks_path = cfg.storage.tasks_path.clone();
    if let Some(parent) = tasks_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ServeError::Engine(format!("任务目录创建失败 {parent:?}: {e}")))?;
        }
    }
    state = state.with_storage(tasks_path.clone());
    // 4b+. 注入生效配置快照（GET /config 返回；精简字段，不含敏感项）
    state = state.with_config(serde_json::json!({
        "dest_root": cfg.download.dest_root,
        "bt_save_path": cfg.bt_save_path(),
        "bt_enabled": cfg.bt.enabled,
        "listen_addr": cfg.server.addr,
        "persist_path": tasks_path,
    }));
    if tasks_path.exists() {
        match state.restore_from(&tasks_path).await {
            Ok(n) => tracing::info!("已从 {tasks_path:?} 恢复 {n} 个任务"),
            Err(e) => tracing::warn!("任务恢复失败（继续空启动）: {e}"),
        }
    }

    // 4c. BT alert 事件流（feature bt 且 BT 启用时）
    #[cfg(feature = "bt")]
    let (alert_handle, state_arc) = {
        let state_arc = Arc::new(state);
        let handle = bt_core.map(|core| {
            crate::bt_events::spawn_alert_loop(state_arc.clone(), core, Duration::from_millis(500))
        });
        (handle, state_arc)
    };
    #[cfg(not(feature = "bt"))]
    let state_arc = Arc::new(state);

    // 5. 路由 + 监听
    let app = http::router(state_arc);
    let listener = tokio::net::TcpListener::bind(&cfg.server.addr)
        .await
        .map_err(|e| ServeError::Bind(cfg.server.addr.clone(), e))?;
    tracing::info!("smart-dl-daemon 监听 {}", cfg.server.addr);

    // 6. 优雅退出（Ctrl+C → 停服 → alert loop 随进程结束）
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = tx.send(());
    });

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = rx.await;
            tracing::info!("收到退出信号，优雅关闭…");
        })
        .await
        .map_err(ServeError::Io)?;

    #[cfg(feature = "bt")]
    if let Some(h) = alert_handle {
        h.abort(); // 进程退出前停止 alert 轮询（锁随 _lock drop 释放）
    }

    Ok(())
}

/// 进程参数：`serve [--config <path>]`。
pub fn parse_args(args: &[String]) -> Result<Option<std::path::PathBuf>, String> {
    let mut cfg_path = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" | "-c" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--config 缺少路径".to_string())?;
                cfg_path = Some(std::path::PathBuf::from(v));
                i += 2;
            }
            a if a.starts_with('-') => return Err(format!("未知参数: {a}")),
            _ => i += 1,
        }
    }
    Ok(cfg_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_flag() {
        let p = parse_args(&["serve".into(), "--config".into(), "x.toml".into()]).unwrap();
        assert_eq!(p, Some(std::path::PathBuf::from("x.toml")));
    }

    #[test]
    fn parse_short_flag() {
        let p = parse_args(&["-c".into(), "y.toml".into()]).unwrap();
        assert_eq!(p, Some(std::path::PathBuf::from("y.toml")));
    }

    #[test]
    fn parse_default_no_flag() {
        assert_eq!(parse_args(&["serve".into()]).unwrap(), None);
    }

    #[test]
    fn parse_unknown_flag_errors() {
        assert!(parse_args(&["--bogus".into()]).is_err());
    }

    #[test]
    fn parse_missing_value_errors() {
        assert!(parse_args(&["--config".into()]).is_err());
    }
}
