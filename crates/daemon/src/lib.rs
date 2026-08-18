//! 守护进程（M6 交付）：事件协议（D36）/ WS 背压 / CLI（D26）/ 健康（§11）/
//! HTTP API + 任务状态（M2–M5 集成）。

#[cfg(feature = "bt")]
pub mod bt;
pub mod cli;
pub mod events;
pub mod health;
pub mod http;
pub mod state;
pub mod ws;

pub use cli::{Cli, CliCommand, CliError};
pub use events::SchedulerEvent;
pub use ws::WsHub;
