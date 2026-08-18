//! smart-dl-daemon 二进制入口：`smart-dl-daemon serve [--config <path>]`。
//! 默认配置见 crate::config（无需配置文件即可启动）。

use smart_dl_daemon::serve;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let cfg_path = match serve::parse_args(&args[1..]) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("参数错误: {e}");
            eprintln!("用法: smart-dl-daemon serve [--config <path>]");
            std::process::exit(2);
        }
    };

    let cfg = match smart_dl_daemon::config::Config::load(cfg_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("配置错误: {e}");
            std::process::exit(2);
        }
    };

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime 创建失败");
    if let Err(e) = rt.block_on(serve::run(cfg)) {
        eprintln!("daemon 退出: {e}");
        std::process::exit(1);
    }
}
