//! 测试基建：本地最小 FTP server（tokio 手写，FTP 协议子集）。
//! 支持：USER/PASS 登录、TYPE I 二进制、PASV 被动模式、SIZE、REST 断点、RETR 传输、
//! 前 N 次控制连接回 421（退避重试测试）；记录 REST 偏移与连接数。

// 按测试二进制编译，未使用的构造/helper 属正常
#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone)]
pub struct FtpServerConfig {
    pub size: u64,
    /// 内容（默认 0x5A 填充）。
    pub content: Option<Vec<u8>>,
    /// 前 N 次控制连接回 421（服务不可用）。
    pub reject_421: u32,
    /// RETR 总是回 550（文件不存在）。
    pub retr_550: bool,
}

impl Default for FtpServerConfig {
    fn default() -> Self {
        FtpServerConfig {
            size: 1024,
            content: None,
            reject_421: 0,
            retr_550: false,
        }
    }
}

pub struct FtpTestServer {
    pub addr: SocketAddr,
    /// 每次 RETR 的 REST 偏移（续传验证）。
    pub rest_offsets: Arc<Mutex<Vec<u64>>>,
    /// 控制连接计数。
    pub control_connections: Arc<AtomicUsize>,
    /// RETR 请求计数。
    pub retr_count: Arc<AtomicUsize>,
}

impl FtpTestServer {
    pub async fn start(cfg: FtpServerConfig) -> Self {
        let rest_offsets = Arc::new(Mutex::new(Vec::new()));
        let control_connections = Arc::new(AtomicUsize::new(0));
        let retr_count = Arc::new(AtomicUsize::new(0));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (ro, cc, rc) = (
            rest_offsets.clone(),
            control_connections.clone(),
            retr_count.clone(),
        );
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let (ro, cc, rc) = (ro.clone(), cc.clone(), rc.clone());
                let cfg = cfg.clone();
                tokio::spawn(async move {
                    handle_control(stream, cfg, ro, cc, rc).await;
                });
            }
        });
        FtpTestServer {
            addr,
            rest_offsets,
            control_connections,
            retr_count,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("ftp://user:pass@{}:{}", self.addr.ip(), self.addr.port()) + path
    }
}

async fn handle_control(
    mut stream: TcpStream,
    cfg: FtpServerConfig,
    rest_offsets: Arc<Mutex<Vec<u64>>>,
    control_connections: Arc<AtomicUsize>,
    retr_count: Arc<AtomicUsize>,
) {
    let conn_no = control_connections.fetch_add(1, Ordering::SeqCst);
    if (conn_no as u32) < cfg.reject_421 {
        let _ = stream.write_all(b"421 Service not available\r\n").await;
        return;
    }
    let _ = stream.write_all(b"220 test ftp ready\r\n").await;

    let mut reader = BufReader::new(stream);
    let mut rest: u64 = 0;
    let mut data_listener: Option<TcpListener> = None;
    let mut body: Option<Vec<u8>> = None; // RETR 数据（懒构建）

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let cmd = line.trim_end_matches("\r\n").trim_end();
        let (verb, arg) = match cmd.split_once(' ') {
            Some((v, a)) => (v, a.trim()),
            None => (cmd, ""),
        };
        let conn = reader.get_mut();
        match verb {
            "USER" | "PASS" => {
                let _ = conn.write_all(b"230 logged in\r\n").await;
            }
            "TYPE" => {
                let _ = conn.write_all(b"200 type set\r\n").await;
            }
            "PASV" => {
                // 数据监听：bind 随机端口
                let dl = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let da = dl.local_addr().unwrap();
                let p1 = da.port() / 256;
                let p2 = da.port() % 256;
                let resp = format!("227 Entering Passive Mode (127,0,0,1,{},{})\r\n", p1, p2);
                let _ = conn.write_all(resp.as_bytes()).await;
                data_listener = Some(dl);
            }
            "SIZE" => {
                let _ = conn
                    .write_all(format!("213 {}\r\n", cfg.size).as_bytes())
                    .await;
            }
            "REST" => {
                if let Ok(n) = arg.parse::<u64>() {
                    rest = n;
                    let _ = conn.write_all(b"350 Restarting at position\r\n").await;
                } else {
                    let _ = conn.write_all(b"501 bad REST\r\n").await;
                }
            }
            "RETR" => {
                rest_offsets.lock().unwrap().push(rest);
                retr_count.fetch_add(1, Ordering::SeqCst);
                if cfg.retr_550 {
                    let _ = conn.write_all(b"550 file unavailable\r\n").await;
                    continue;
                }
                if body.is_none() {
                    body = Some(
                        cfg.content
                            .clone()
                            .unwrap_or_else(|| vec![0x5Au8; cfg.size as usize]),
                    );
                }
                let b = body.as_ref().unwrap();
                if rest > b.len() as u64 {
                    let _ = conn.write_all(b"550 file unavailable\r\n").await;
                    continue;
                }
                let _ = conn.write_all(b"150 opening data connection\r\n").await;
                if let Some(dl) = data_listener.as_ref() {
                    if let Ok((mut data, _)) = dl.accept().await {
                        let chunk = &b[rest as usize..];
                        let _ = data.write_all(chunk).await;
                        let _ = data.shutdown().await;
                    }
                }
                let _ = conn.write_all(b"226 transfer complete\r\n").await;
                rest = 0;
                data_listener = None;
            }
            "QUIT" => {
                let _ = conn.write_all(b"221 bye\r\n").await;
                return;
            }
            _ => {
                let _ = conn.write_all(b"502 unknown\r\n").await;
            }
        }
    }
}

/// 便捷：确定性内容（与 http_server 的 patterned 同构）。
pub fn patterned(size: u64) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}
