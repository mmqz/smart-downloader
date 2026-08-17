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
