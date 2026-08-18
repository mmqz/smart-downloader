//! M6: CLI 命令集（D26）——8 命令 add/pause/resume/remove/list/status/logs/config
//! + fallback 手动兜底（Q-B9 入口）+ --json 输出。

use smart_dl_daemon::cli::{Cli, CliCommand, CliError};

fn parse(args: &[&str]) -> Result<CliCommand, CliError> {
    let v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    Cli::parse(&v)
}

#[test]
fn parse_add_http_url() {
    match parse(&["smart-dl", "add", "http://x/file.bin"]).unwrap() {
        CliCommand::Add { url, dest } => {
            assert_eq!(url, "http://x/file.bin");
            assert_eq!(dest, None);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_add_with_dest_and_json() {
    match parse(&["smart-dl", "--json", "add", "http://x/f", "-o", "out/"]).unwrap() {
        CliCommand::Add { url, dest: Some(d) } => {
            assert_eq!(url, "http://x/f");
            assert_eq!(d, "out/");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_lifecycle_commands() {
    assert_eq!(
        parse(&["smart-dl", "pause", "t1"]).unwrap(),
        CliCommand::Pause {
            task_id: "t1".into()
        }
    );
    assert_eq!(
        parse(&["smart-dl", "resume", "t1"]).unwrap(),
        CliCommand::Resume {
            task_id: "t1".into()
        }
    );
    assert_eq!(
        parse(&["smart-dl", "remove", "t1"]).unwrap(),
        CliCommand::Remove {
            task_id: "t1".into()
        }
    );
    assert_eq!(
        parse(&["smart-dl", "status", "t1"]).unwrap(),
        CliCommand::Status {
            task_id: "t1".into()
        }
    );
}

#[test]
fn parse_list_logs_config() {
    assert_eq!(parse(&["smart-dl", "list"]).unwrap(), CliCommand::List);
    assert_eq!(
        parse(&["smart-dl", "logs", "t1"]).unwrap(),
        CliCommand::Logs {
            task_id: "t1".into()
        }
    );
    assert_eq!(parse(&["smart-dl", "config"]).unwrap(), CliCommand::Config);
}

#[test]
fn parse_fallback_manual_command() {
    // Q-B9 手动兜底入口：fallback <task_id>
    match parse(&["smart-dl", "fallback", "t9"]).unwrap() {
        CliCommand::Fallback { task_id } => assert_eq!(task_id, "t9"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn json_flag_exposed_on_cli() {
    let args = |a: &[&str]| {
        let v: Vec<String> = a.iter().map(|s| s.to_string()).collect();
        Cli::from_args(&v).unwrap()
    };
    assert!(
        args(&["smart-dl", "--json", "list"]).json,
        "--json 全局标志"
    );
    assert!(!args(&["smart-dl", "list"]).json);
}

#[test]
fn unknown_command_rejected() {
    assert!(matches!(
        parse(&["smart-dl", "explode"]),
        Err(CliError::Unknown(_))
    ));
}

#[test]
fn missing_argument_rejected() {
    assert!(matches!(
        parse(&["smart-dl", "pause"]),
        Err(CliError::MissingArg(_))
    ));
}

// —— 执行层集成：真实 serve + CLI 客户端往返 ——

mod common;

use smart_dl_daemon::client::CliClient;
use smart_dl_daemon::http;
use smart_dl_daemon::state::DaemonState;
use std::sync::Arc;

#[tokio::test]
async fn cli_add_list_status_roundtrip() {
    let engine = smart_dl_httpdl::HttpEngine::new(reqwest::Client::new());
    let state = Arc::new(DaemonState::new(Arc::new(engine), vec![]));
    let app = http::router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = CliClient::new(&base);

    let srv = common::TestServer::start(common::patterned(1024)).await;
    let url = srv.url();

    // add → 成功（默认 dest）
    client
        .run(
            &CliCommand::Add {
                url: url.clone(),
                dest: None,
            },
            false,
        )
        .await
        .unwrap();

    // 重复 add 同 URL → 409 → Err（错误信息含 duplicate）
    let dup = client
        .run(
            &CliCommand::Add {
                url: url.clone(),
                dest: None,
            },
            false,
        )
        .await;
    assert!(dup.is_err(), "重复 add 应报错 409: {dup:?}");
    assert!(dup.unwrap_err().to_string().contains("duplicate"));

    // list → 任务 t1 在列
    client.run(&CliCommand::List, false).await.unwrap();

    // status t1 → 字段齐全
    client
        .run(
            &CliCommand::Status {
                task_id: "t1".into(),
            },
            false,
        )
        .await
        .unwrap();

    // remove → 清除
    client
        .run(
            &CliCommand::Remove {
                task_id: "t1".into(),
            },
            false,
        )
        .await
        .unwrap();
    // 删除后再查 → 404 → Err
    let gone = client
        .run(
            &CliCommand::Status {
                task_id: "t1".into(),
            },
            false,
        )
        .await;
    assert!(gone.is_err(), "删除后 status 应 404: {gone:?}");
}

#[tokio::test]
async fn cli_json_output_success() {
    let engine = smart_dl_httpdl::HttpEngine::new(reqwest::Client::new());
    let state = Arc::new(DaemonState::new(Arc::new(engine), vec![]));
    let app = http::router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = CliClient::new(&base);

    // 空列表 --json 输出
    client.run(&CliCommand::List, true).await.unwrap();

    // 未实现命令 → 明确提示
    let cfg = client.run(&CliCommand::Config, false).await;
    assert!(cfg.is_err());
    assert!(cfg.unwrap_err().to_string().contains("无对应端点"));
}
