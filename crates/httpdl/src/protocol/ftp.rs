//! FTP 协议子集（M4c，feature=`ftp`，设计 §15）：
//! 被动模式（PASV）、REST 断点续传（.part）、421 退避重试；
//! 不支持 SFTP/FTPS 隐式/目录递归/FXP。

use crate::retry::Backoff;
use crate::static_split::plan_segments;
use smart_dl_core::session::output::OutputManager;
use smart_dl_core::task::DownloadTask;
use smart_dl_core::types::{
    Capability, DownloadEngine, DownloadSource, EngineError, EngineKind, EngineState, EngineStatus,
    EngineTaskId, PeerInfo,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// 421/连接失败重试次数（连接层退避）。
const CONNECT_ATTEMPTS: u32 = 4;

struct FtpTask {
    host: String,
    port: u16,
    user: String,
    pass: String,
    path: String,
    dest: PathBuf,
    total: u64,
    state: EngineState,
    done: u64,
    error: Option<String>,
}

struct EngineInner {
    tasks: Mutex<HashMap<EngineTaskId, FtpTask>>,
}

/// FTP 引擎（串行段下载：PASV + REST + RETR）。
#[derive(Clone)]
pub struct FtpEngine {
    backoff: Backoff,
    inner: Arc<EngineInner>,
}

impl FtpEngine {
    pub fn new() -> Self {
        FtpEngine::with_backoff(Backoff::default())
    }

    /// 可注入退避（421 测试用短退避）。
    pub fn with_backoff(backoff: Backoff) -> Self {
        FtpEngine {
            backoff,
            inner: Arc::new(EngineInner {
                tasks: Mutex::new(HashMap::new()),
            }),
        }
    }
}

impl Default for FtpEngine {
    fn default() -> Self {
        FtpEngine::new()
    }
}

/// 解析 `ftp://[user:pass@]host[:port]/path`。目录（空路径或以 / 结尾）→ None。
fn parse_ftp_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("ftp://")?;
    let (auth_host, path) = rest.split_once('/')?;
    let path = format!("/{path}");
    if path == "/" || path.ends_with('/') {
        return None; // 目录（v1 不支持）
    }
    let host_port = auth_host
        .rsplit_once('@')
        .map(|(_, hp)| hp)
        .unwrap_or(auth_host);
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (host_port.to_string(), 21),
    };
    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
}

/// 控制连接会话（单命令/响应流）。
struct FtpSession {
    reader: BufReader<TcpStream>,
}

impl FtpSession {
    async fn connect(host: &str, port: u16) -> Result<Self, String> {
        let stream = TcpStream::connect((host, port))
            .await
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        let banner = read_response(&mut reader).await?;
        if banner.starts_with("421") {
            return Err(format!("421 {banner}"));
        }
        Ok(FtpSession { reader })
    }

    async fn login(&mut self, user: &str, pass: &str) -> Result<(), String> {
        self.cmd(&format!("USER {user}")).await?;
        self.cmd(&format!("PASS {pass}")).await?;
        self.cmd("TYPE I").await?;
        Ok(())
    }

    /// PASV → 数据连接地址。
    async fn pasv(&mut self) -> Result<SocketAddr, String> {
        let resp = self.cmd("PASV").await?;
        parse_pasv(&resp)
    }

    async fn size(&mut self, path: &str) -> Result<u64, String> {
        let resp = self.cmd(&format!("SIZE {path}")).await?;
        resp.strip_prefix("213 ")
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| format!("SIZE failed: {resp}"))
    }

    /// 发命令并读单行响应。
    async fn cmd(&mut self, line: &str) -> Result<String, String> {
        let mut buf = line.to_string();
        buf.push_str("\r\n");
        self.reader
            .get_mut()
            .write_all(buf.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        read_response(&mut self.reader).await
    }

    async fn quit(&mut self) {
        let _ = self.cmd("QUIT").await;
    }
}

async fn read_response(reader: &mut BufReader<TcpStream>) -> Result<String, String> {
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .await
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("connection closed by server".to_string());
    }
    Ok(line.trim_end().to_string())
}

/// 解析 `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)`。
fn parse_pasv(resp: &str) -> Result<SocketAddr, String> {
    let nums: Vec<u32> = resp
        .split('(')
        .nth(1)
        .and_then(|s| s.split(')').next())
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .ok_or_else(|| format!("bad PASV: {resp}"))?;
    if nums.len() != 6 {
        return Err(format!("bad PASV tuple: {resp}"));
    }
    Ok(SocketAddr::from((
        [nums[0] as u8, nums[1] as u8, nums[2] as u8, nums[3] as u8],
        (nums[4] * 256 + nums[5]) as u16,
    )))
}

/// 下载一个段（独立连接：连接+登录+PASV+REST+RETR），写入 .part 段位置。
async fn download_segment(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    path: &str,
    seg: crate::static_split::Segment,
    part: &Path,
) -> Result<(), String> {
    let mut s = FtpSession::connect(host, port).await?;
    s.login(user, pass).await?;
    let data_addr = s.pasv().await?;
    s.cmd(&format!("REST {}", seg.start)).await?;
    // RETR 必须是 1xx 中间响应（150）；550 等错误直接终态失败
    let retr = s.cmd(&format!("RETR {path}")).await?;
    if !retr.starts_with('1') {
        return Err(retr);
    }
    let mut data = TcpStream::connect(data_addr)
        .await
        .map_err(|e| e.to_string())?;
    let need = seg.len() as usize;
    let mut buf = vec![0u8; need];
    let mut got = 0usize;
    while got < need {
        let n = data
            .read(&mut buf[got..])
            .await
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err(format!("data connection closed early: {got}/{need}"));
        }
        got += n;
    }
    let _ = read_response(&mut s.reader).await; // 226
    s.quit().await;

    // 写 .part 段位置（段不相交 → 无锁）
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(part)
        .map_err(|e| e.to_string())?;
    use std::io::{Seek, SeekFrom, Write};
    f.seek(SeekFrom::Start(seg.start))
        .map_err(|e| e.to_string())?;
    f.write_all(&buf).map_err(|e| e.to_string())?;
    Ok(())
}

/// 下载循环：串行段下载；连接失败/421 → 退避重试；段终态失败 → Error。
/// 续传：.part 存在（>0 且 < total）→ 单段 REST 从 part 大小续到文件尾；
/// 无 .part（或已满）→ 正常分块下载。
async fn download_loop(inner: Arc<EngineInner>, tid: EngineTaskId, backoff: Backoff) {
    let (host, port, user, pass, path, dest, total) = {
        let tasks = inner.tasks.lock().unwrap();
        let t = tasks.get(&tid).unwrap();
        (
            t.host.clone(),
            t.port,
            t.user.clone(),
            t.pass.clone(),
            t.path.clone(),
            t.dest.clone(),
            t.total,
        )
    };
    let part = part_path_of(&dest);
    let part_done = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let segments = if part_done > 0 && part_done < total {
        // .part 续传：从偏移续到文件尾（单段）
        vec![crate::static_split::Segment {
            start: part_done,
            end: total - 1,
        }]
    } else {
        plan_segments(total)
    };

    // 预分配 .part
    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&part)
        .map(|f| f.set_len(total))
    {
        finish(
            &inner,
            &tid,
            EngineState::Error,
            Some(format!("part open: {e}")),
        );
        return;
    }

    for seg in segments {
        let mut ok = false;
        for attempt in 1..=CONNECT_ATTEMPTS {
            match download_segment(&host, port, &user, &pass, &path, seg, &part).await {
                Ok(()) => {
                    ok = true;
                    break;
                }
                Err(e) => {
                    // 连接层失败（421/IO）→ 退避重试；550 等终态 → 直接失败
                    if attempt < CONNECT_ATTEMPTS && !is_terminal(&e) {
                        tokio::time::sleep(backoff.next_delay(attempt)).await;
                        continue;
                    }
                    finish(&inner, &tid, EngineState::Error, Some(e));
                    return;
                }
            }
        }
        if !ok {
            return;
        }
        // 进度：段完成
        let mut tasks = inner.tasks.lock().unwrap();
        if let Some(t) = tasks.get_mut(&tid) {
            t.done += seg.len();
        }
        drop(tasks);
    }

    // 全部段完成 → 落位
    match finalize_part(&part, &dest, total) {
        Ok(()) => finish(&inner, &tid, EngineState::Completed, None),
        Err(e) => finish(&inner, &tid, EngineState::Error, Some(e)),
    }
}

/// 421/连接类错误可重试；550/协议终态不重试。
fn is_terminal(e: &str) -> bool {
    e.starts_with("550") || e.starts_with("5")
}

fn part_path_of(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

fn finalize_part(part: &Path, dest: &Path, total: u64) -> Result<(), String> {
    let om = OutputManager::new(PathBuf::from("."));
    om.finalize_to(part, dest, total).map_err(|e| e.to_string())
}

fn finish(inner: &Arc<EngineInner>, tid: &str, state: EngineState, error: Option<String>) {
    let mut tasks = inner.tasks.lock().unwrap();
    if let Some(t) = tasks.get_mut(tid) {
        t.state = state;
        t.error = error;
        if state == EngineState::Completed {
            t.done = t.total;
        }
    }
}

#[async_trait::async_trait]
impl DownloadEngine for FtpEngine {
    fn id(&self) -> &str {
        "ftp"
    }

    fn kind(&self) -> EngineKind {
        EngineKind::Ftp
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Ftp, Capability::FtpResume]
    }

    async fn add(&self, task: &DownloadTask) -> Result<EngineTaskId, EngineError> {
        let (url, user, pass) = match &task.source {
            DownloadSource::Ftp { url, user, pass } => (url.clone(), user.clone(), pass.clone()),
            _ => return Err(EngineError::Other("source is not ftp".to_string())),
        };
        let (host, port, path) = parse_ftp_url(&url).ok_or_else(|| {
            EngineError::Other("invalid ftp url or directory (v1 unsupported)".to_string())
        })?;

        // 探测：连接 + 登录 + SIZE（421 → 退避重试）
        let total = {
            let mut last = String::new();
            let mut size: Option<u64> = None;
            for attempt in 1..=CONNECT_ATTEMPTS {
                match probe_size(&host, port, &user, &pass, &path).await {
                    Ok(t) => {
                        size = Some(t);
                        break;
                    }
                    Err(e) => {
                        last = e;
                        if attempt < CONNECT_ATTEMPTS && !is_terminal(&last) {
                            tokio::time::sleep(self.backoff.next_delay(attempt)).await;
                            continue;
                        }
                        break;
                    }
                }
            }
            match size {
                Some(t) => t,
                None => return Err(EngineError::Other(format!("ftp probe failed: {last}"))),
            }
        };

        let rel = task
            .metadata
            .name
            .clone()
            .unwrap_or_else(|| "download.bin".to_string());
        let dest = task.dest_root.join(&rel);
        // .part 超长（源变小）→ 作废
        let part = part_path_of(&dest);
        if let Ok(md) = std::fs::metadata(&part) {
            if md.len() > total {
                let _ = std::fs::remove_file(&part);
            }
        }

        let tid = task.id.clone();
        {
            let mut tasks = self.inner.tasks.lock().unwrap();
            tasks.insert(
                tid.clone(),
                FtpTask {
                    host,
                    port,
                    user,
                    pass,
                    path,
                    dest,
                    total,
                    state: EngineState::Downloading,
                    done: part_done(&part),
                    error: None,
                },
            );
        }
        let inner = self.inner.clone();
        let backoff = self.backoff;
        let spawn_tid = tid.clone();
        tokio::spawn(async move {
            download_loop(inner, spawn_tid, backoff).await;
        });
        Ok(tid)
    }

    async fn pause(&self, id: &EngineTaskId) -> Result<(), EngineError> {
        let mut tasks = self.inner.tasks.lock().unwrap();
        let t = tasks.get_mut(id).ok_or(EngineError::NotFound)?;
        t.state = EngineState::Paused;
        Ok(())
    }

    async fn resume(&self, id: &EngineTaskId) -> Result<(), EngineError> {
        let mut tasks = self.inner.tasks.lock().unwrap();
        let t = tasks.get_mut(id).ok_or(EngineError::NotFound)?;
        t.state = EngineState::Downloading;
        Ok(())
    }

    async fn status(&self, id: &EngineTaskId) -> Result<EngineStatus, EngineError> {
        let tasks = self.inner.tasks.lock().unwrap();
        let t = tasks.get(id).ok_or(EngineError::NotFound)?;
        Ok(EngineStatus {
            state: t.state,
            metadata_received: true,
            files: vec![],
            total_done: t.done,
            total: t.total,
            down_rate: 0,
            up_rate: 0,
            num_peers: 0,
            num_seeds: 0,
            error: t.error.clone(),
        })
    }

    async fn remove(&self, id: &EngineTaskId, _delete_data: bool) -> Result<(), EngineError> {
        let mut tasks = self.inner.tasks.lock().unwrap();
        tasks.remove(id).ok_or(EngineError::NotFound)?;
        Ok(())
    }

    async fn peers(&self, _id: &EngineTaskId) -> Result<Vec<PeerInfo>, EngineError> {
        Ok(vec![])
    }

    async fn update_sources(
        &self,
        _id: &EngineTaskId,
        _urls: Vec<String>,
    ) -> Result<(), EngineError> {
        Ok(())
    }

    async fn add_url_seed(&self, _id: &EngineTaskId, _url: &str) -> Result<(), EngineError> {
        Ok(())
    }

    async fn ban_peer(&self, _id: &EngineTaskId, _peer: SocketAddr) -> Result<(), EngineError> {
        Ok(())
    }

    async fn read_piece(&self, _id: &EngineTaskId, _idx: u32) -> Result<Vec<u8>, EngineError> {
        Err(EngineError::Unsupported)
    }
}

/// 探测文件大小：连接 + 登录 + SIZE。
async fn probe_size(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    path: &str,
) -> Result<u64, String> {
    let mut s = FtpSession::connect(host, port).await?;
    s.login(user, pass).await?;
    let size = s.size(path).await;
    s.quit().await;
    size
}

/// 现有 .part 已下载字节数（续传起点）。
fn part_done(part: &Path) -> u64 {
    std::fs::metadata(part).map(|m| m.len()).unwrap_or(0)
}
