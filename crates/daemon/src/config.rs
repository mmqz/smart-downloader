//! serve 配置（TOML）：HTTP 监听地址 / 默认下载目录 / BT 引擎开关 / 单实例锁路径。
//! 文件缺失时使用默认值（Config::default）；`--config <path>` 覆盖。

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerCfg,
    pub download: DownloadCfg,
    pub bt: BtCfg,
    pub lock: LockCfg,
    pub storage: StorageCfg,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ServerCfg {
    /// HTTP/WS 监听地址，如 `127.0.0.1:8787`。
    pub addr: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DownloadCfg {
    /// 默认下载落盘根目录（三 add 入口的 dest 缺省值）。
    pub dest_root: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BtCfg {
    /// 启用 BT 引擎（需编译时 --features bt）。
    pub enabled: bool,
    /// BT 落盘目录（须存在；默认与 dest_root 相同）。
    pub save_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LockCfg {
    /// 单实例锁文件路径（重复启动 → 拒绝）。
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StorageCfg {
    /// 任务持久化文件（add/remove/状态变更自动落盘；启动时恢复）。
    pub tasks_path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerCfg {
                addr: "127.0.0.1:8787".into(),
            },
            download: DownloadCfg {
                dest_root: PathBuf::from("./downloads"),
            },
            bt: BtCfg {
                enabled: true,
                save_path: None,
            },
            lock: LockCfg {
                path: PathBuf::from("./daemon.lock"),
            },
            storage: StorageCfg {
                tasks_path: PathBuf::from("./tasks.json"),
            },
        }
    }
}

impl Config {
    /// 从 TOML 文件加载；文件不存在 → 默认值；解析失败 → Err（含行号）。
    pub fn load(path: Option<&std::path::Path>) -> Result<Config, String> {
        let Some(p) = path else {
            return Ok(Config::default());
        };
        let text = std::fs::read_to_string(p).map_err(|e| format!("读取配置 {p:?} 失败: {e}"))?;
        toml::from_str(&text).map_err(|e| format!("配置解析失败 {p:?}: {e}"))
    }

    /// BT 实际落盘目录（save_path 或默认 dest_root）。
    pub fn bt_save_path(&self) -> PathBuf {
        self.bt
            .save_path
            .clone()
            .unwrap_or_else(|| self.download.dest_root.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let c = Config::default();
        assert_eq!(c.server.addr, "127.0.0.1:8787");
        assert_eq!(c.download.dest_root, PathBuf::from("./downloads"));
        assert!(c.bt.enabled);
        assert_eq!(c.lock.path, PathBuf::from("./daemon.lock"));
        assert_eq!(c.bt_save_path(), c.download.dest_root);
    }

    #[test]
    fn parse_toml_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(
            &p,
            r#"
[server]
addr = "0.0.0.0:9999"

[download]
dest_root = "/data/dl"

[bt]
enabled = false
save_path = "/data/bt"

[lock]
path = "/tmp/sd.lock"
"#,
        )
        .unwrap();
        let c = Config::load(Some(&p)).unwrap();
        assert_eq!(c.server.addr, "0.0.0.0:9999");
        assert_eq!(c.download.dest_root, PathBuf::from("/data/dl"));
        assert!(!c.bt.enabled);
        assert_eq!(c.bt_save_path(), PathBuf::from("/data/bt"));
        assert_eq!(c.lock.path, PathBuf::from("/tmp/sd.lock"));
    }

    #[test]
    fn missing_file_uses_default() {
        assert_eq!(Config::load(None).unwrap().server.addr, "127.0.0.1:8787");
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        assert!(
            Config::load(Some(&missing)).is_err(),
            "缺失文件应报错（显式路径）"
        );
    }

    #[test]
    fn partial_toml_keeps_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[server]\naddr = \"127.0.0.1:8080\"\n").unwrap();
        let c = Config::load(Some(&p)).unwrap();
        assert_eq!(c.server.addr, "127.0.0.1:8080");
        assert_eq!(
            c.download.dest_root,
            PathBuf::from("./downloads"),
            "未写字段用默认"
        );
    }

    #[test]
    fn bad_toml_reports_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "server = [unclosed").unwrap();
        let err = Config::load(Some(&p)).unwrap_err();
        assert!(err.contains("解析失败"), "应报解析错误: {err}");
    }
}
