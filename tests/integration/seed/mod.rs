//! tests/integration/seed — 本地测试 seeder（自研 seed_main：生成 2MB 确定性文件并做种）
//! 约定：seed_main <port> <dir> 启动后输出一行 `SEED <magnet> PORT <port>`，随后常驻。

use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub struct TestSeeder {
    child: Child,
    magnet: String,
    port: u16,
    dir: TempDir,
}

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("smart-dl-test-{}-{}", std::process::id(), nanos));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

static SEED_MAIN: OnceLock<Option<PathBuf>> = OnceLock::new();

fn seed_main_path() -> Option<PathBuf> {
    SEED_MAIN
        .get_or_init(|| {
            std::env::var("SEED_MAIN").ok().map(PathBuf::from).or_else(|| {
                let root = std::env::current_dir().ok()?;
                let mut p = root;
                loop {
                    let cand = p.join("ffi").join("build").join("Release").join("seed_main.exe");
                    if cand.exists() {
                        return Some(cand);
                    }
                    if !p.pop() {
                        return None;
                    }
                }
            })
        })
        .clone()
}

fn pick_free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("bind ephemeral")
        .local_addr()
        .expect("local_addr")
        .port()
}

impl TestSeeder {
    /// 启动 seed_main；要求 02_build.ps1 已产出 seed_main.exe（M0 出口前置）
    /// stdout 重定向到临时文件（避免管道不被排空导致 seed_main 阻塞自身 session，
    /// 令 peer 连接全部断开——M0 e2e 调试实测）。
    pub fn start() -> Self {
        let exe = seed_main_path().expect("seed_main.exe 未构建：先跑 scripts/m0/02_build.ps1");
        let port = pick_free_port();
        let dir = TempDir::new().expect("tempdir");
        let save: PathBuf = dir.path().to_path_buf(); // 由 seeder 写入 2MB 测试文件
        let log = dir.path().join("seed.log");
        let logf = std::fs::File::create(&log).expect("create seed.log");

        let mut child = Command::new(exe)
            .arg(port.to_string())
            .arg(&save)
            .stdout(Stdio::from(logf))
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn seed_main");

        // 30s 内从文件读到 "SEED <magnet> PORT <port>"
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(f) = std::fs::File::open(&log) {
                let mut lines = BufReader::new(f).lines();
                while let Some(Ok(line)) = lines.next() {
                    let t = line.trim().to_string();
                    if t.starts_with("SEED ") {
                        let mut parts = t.split_whitespace();
                        parts.next(); // SEED
                        let magnet = parts.next().expect("magnet").to_string();
                        parts.next(); // PORT
                        let p: u16 = parts.next().expect("port").parse().expect("port num");
                        assert_eq!(p, port, "seed_main 报告的端口与请求不一致");
                        return Self { child, magnet, port, dir };
                    }
                }
            }
            if child.try_wait().ok().flatten().is_some() {
                let _ = child.kill();
                let tail = std::fs::read_to_string(&log).unwrap_or_default();
                panic!("seed_main 提前退出: {}", tail);
            }
            assert!(Instant::now() < deadline, "seed_main 30s 内未输出 SEED 行");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn magnet(&self) -> &str {
        &self.magnet
    }
    /// (ip, port) 供 lt_add_peer 直连注入
    pub fn addr(&self) -> (String, u16) {
        (SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)).ip().to_string(), self.port)
    }
    /// seeder 诊断输出（seed_main 打印的 SEED-STATUS/SEED-ALERT 行）
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("seed.log")).unwrap_or_default()
    }
}

impl Drop for TestSeeder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}