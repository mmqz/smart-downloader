//! CLI 命令集（D26）：add/pause/resume/remove/list/status/logs/config
//! + fallback <task_id>（Q-B9 手动兜底入口）+ --json 全局输出标志。

/// 解析结果（--json 标志 + 命令）。
#[derive(Clone, Debug, PartialEq)]
pub struct Cli {
    pub json: bool,
    pub command: CliCommand,
}

/// 命令。
#[derive(Clone, Debug, PartialEq)]
pub enum CliCommand {
    Add {
        url: String,
        dest: Option<String>,
    },
    Pause {
        task_id: String,
    },
    Resume {
        task_id: String,
    },
    Remove {
        task_id: String,
    },
    List,
    Status {
        task_id: String,
    },
    Logs {
        task_id: String,
    },
    Config,
    /// Q-B9 手动兜底（metadata 超时标志后的人工入口）。
    Fallback {
        task_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    #[error("unknown command: {0}")]
    Unknown(String),
    #[error("missing argument for {0}")]
    MissingArg(String),
    #[error("http: {0}")]
    Http(String),
}

impl Cli {
    /// 从完整 argv 解析（含程序名；过滤 --json 全局标志）。
    pub fn from_args(args: &[String]) -> Result<Cli, CliError> {
        let mut json = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            if a == "--json" {
                json = true;
            } else {
                rest.push(a.clone());
            }
        }
        let command = parse_command(&rest[1..])?;
        Ok(Cli { json, command })
    }

    /// 解析为单个命令（测试友好：直接传命令参数）。
    pub fn parse(args: &[String]) -> Result<CliCommand, CliError> {
        Ok(Cli::from_args(args)?.command)
    }
}

fn parse_command(args: &[String]) -> Result<CliCommand, CliError> {
    let cmd = args
        .first()
        .ok_or_else(|| CliError::MissingArg("command".to_string()))?;
    match cmd.as_str() {
        "add" => {
            let url = args
                .get(1)
                .ok_or_else(|| CliError::MissingArg("add <url>".to_string()))?
                .clone();
            let dest = match args.get(2).map(|s| s.as_str()) {
                Some("-o") => Some(
                    args.get(3)
                        .ok_or_else(|| CliError::MissingArg("-o <dir>".to_string()))?
                        .clone(),
                ),
                _ => None,
            };
            Ok(CliCommand::Add { url, dest })
        }
        "pause" | "resume" | "remove" | "status" | "logs" | "fallback" => {
            let task_id = args
                .get(1)
                .ok_or_else(|| CliError::MissingArg(format!("{cmd} <task_id>")))?;
            Ok(match cmd.as_str() {
                "pause" => CliCommand::Pause {
                    task_id: task_id.clone(),
                },
                "resume" => CliCommand::Resume {
                    task_id: task_id.clone(),
                },
                "remove" => CliCommand::Remove {
                    task_id: task_id.clone(),
                },
                "status" => CliCommand::Status {
                    task_id: task_id.clone(),
                },
                "logs" => CliCommand::Logs {
                    task_id: task_id.clone(),
                },
                "fallback" => CliCommand::Fallback {
                    task_id: task_id.clone(),
                },
                _ => unreachable!(),
            })
        }
        "list" => Ok(CliCommand::List),
        "config" => Ok(CliCommand::Config),
        other => Err(CliError::Unknown(other.to_string())),
    }
}
