//! BtEngine（feature `bt`）：libtorrent 薄核（smart-dl-btcore）接入 DownloadEngine。
//!
//! 单个 BtCore session（save_path = 任务默认落盘目录）；engine_tid = libtorrent
//! 返回的 infohash（40 hex）。magnet / .torrent 文件 → add_magnet / add_torrent_file。
//! 状态映射：lt state 0 下载 / 1 完成 / 3 错误 / 4 元数据获取中（ABI100 无暂停态，
//! 暂停以 alert 同步——v1 用 pause/resume 直调，状态以 status 为准）。

use smart_dl_btcore::{BtCore, TorrentStatus};
use smart_dl_core::task::DownloadTask;
use smart_dl_core::types::{
    Capability, DownloadEngine, DownloadSource, EngineError, EngineKind, EngineState, EngineStatus,
    EngineTaskId, FileProgress, PeerInfo,
};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const LT_STATE_COMPLETED: i32 = 1;
const LT_STATE_ERROR: i32 = 3;
const LT_STATE_METADATA: i32 = 4;

fn map_state(st: i32) -> EngineState {
    match st {
        LT_STATE_METADATA => EngineState::MetadataPending,
        LT_STATE_COMPLETED => EngineState::Seeding,
        LT_STATE_ERROR => EngineState::Error,
        _ => EngineState::Downloading,
    }
}

fn map_status(st: &TorrentStatus) -> EngineStatus {
    let state = map_state(st.state);
    let total = st.total.max(0) as u64;
    let mut es = EngineStatus {
        state,
        metadata_received: st.metadata_received,
        files: vec![],
        total_done: st.downloaded.max(0) as u64,
        total,
        down_rate: st.down_rate.max(0) as u64,
        up_rate: st.up_rate.max(0) as u64,
        num_peers: st.num_peers.max(0) as u32,
        num_seeds: st.num_seeds.max(0) as u32,
        error: (state == EngineState::Error).then(|| "bt error".to_string()),
    };
    if total > 0 {
        es.files.push(FileProgress {
            rel_path: String::new(),
            done: st.downloaded.max(0) as u64,
            size: total,
        });
    }
    es
}

/// libtorrent 薄核引擎（单 session）。
/// **落盘语义（v1）**：单 session 全局落盘目录（`BtEngine::new` 的 save_path，serve 配置
/// `[bt] save_path`）。`DownloadTask.dest_root` 仅接受与全局目录一致或默认 `"."`——
/// 显式指定其他目录会返回错误（避免"用户指定 A 目录、实际落 B 目录"的静默错位）。
/// **恢复续传**：重启后同一 save_path 重新 add 同一 magnet/torrent → libtorrent 磁盘检查复用
/// 已下载块（无需 fastresume；resume 数据未接，checking 全盘但功能正确）。
pub struct BtEngine {
    core: Arc<BtCore>,
    save_path: PathBuf,
}

impl BtEngine {
    /// 新建 BT 会话（save_path 为全局落盘目录，须已存在）。
    pub fn new(save_path: &Path) -> Result<Self, String> {
        BtCore::new(save_path, "smart-dl-daemon")
            .map(|core| BtEngine {
                core: Arc::new(core),
                save_path: save_path.to_path_buf(),
            })
            .map_err(|e| format!("bt session init: {}", core_err(&e)))
    }

    pub fn core(&self) -> Arc<BtCore> {
        self.core.clone()
    }
}

fn core_err(e: &smart_dl_btcore::Error) -> String {
    format!("{:?}", e)
}

#[async_trait::async_trait]
impl DownloadEngine for BtEngine {
    fn id(&self) -> &str {
        "bt"
    }

    fn kind(&self) -> EngineKind {
        EngineKind::Bt
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::Magnet,
            Capability::TorrentFile,
            Capability::Peer,
            Capability::Tracker,
            Capability::Dht,
            Capability::WebSeed,
            Capability::PieceRead,
        ]
    }

    async fn add(&self, task: &DownloadTask) -> Result<EngineTaskId, EngineError> {
        // v1 落盘约束：任务级 dest 仅接受默认 "." 或与全局 save_path 一致
        if task.dest_root != Path::new(".") && task.dest_root != self.save_path {
            return Err(EngineError::Other(format!(
                "BT 引擎 v1 全局落盘于 {:?}，任务 dest {:#?} 不支持（请用全局目录或默认）",
                self.save_path, task.dest_root
            )));
        }
        let web_seeds: Vec<String> = vec![];
        let ih = match &task.source {
            DownloadSource::Magnet(m) => self.core.add_magnet(m, &web_seeds),
            DownloadSource::TorrentFile(bytes) => self.core.add_torrent_file(bytes, &web_seeds),
            _ => return Err(EngineError::Other("source is not bt".to_string())),
        };
        ih.map_err(|e| EngineError::Other(core_err(&e)))
    }

    async fn pause(&self, id: &EngineTaskId) -> Result<(), EngineError> {
        self.core
            .pause(id)
            .map_err(|e| EngineError::Other(core_err(&e)))
    }

    async fn resume(&self, id: &EngineTaskId) -> Result<(), EngineError> {
        self.core
            .resume(id)
            .map_err(|e| EngineError::Other(core_err(&e)))
    }

    async fn status(&self, id: &EngineTaskId) -> Result<EngineStatus, EngineError> {
        // 会话内未注册的 infohash → NotFound（任务已移除/从未添加）。
        match self.core.status(id) {
            Ok(st) => Ok(map_status(&st)),
            Err(_) => Err(EngineError::NotFound),
        }
    }

    async fn remove(&self, id: &EngineTaskId, delete_data: bool) -> Result<(), EngineError> {
        self.core
            .remove(id, delete_data)
            .map_err(|e| EngineError::Other(core_err(&e)))
    }

    async fn peers(&self, id: &EngineTaskId) -> Result<Vec<PeerInfo>, EngineError> {
        self.core
            .peers(id)
            .map(|ps| {
                ps.into_iter()
                    .map(|p| PeerInfo {
                        ip: p.ip,
                        port: p.port,
                        peer_id: p.peer_id,
                        client: p.client,
                        progress_ppm: p.progress_ppm,
                        down_rate: p.down_rate.max(0) as u64,
                        up_rate: p.up_rate.max(0) as u64,
                        total_download: p.total_download.max(0) as u64,
                        total_upload: p.total_upload.max(0) as u64,
                        last_active_sec: p.last_active_sec.max(0) as u64,
                        flags: format!("{:08x}", p.flags),
                    })
                    .collect()
            })
            .map_err(|e| EngineError::Other(core_err(&e)))
    }

    async fn update_sources(
        &self,
        _id: &EngineTaskId,
        _urls: Vec<String>,
    ) -> Result<(), EngineError> {
        Err(EngineError::Unsupported)
    }

    async fn add_url_seed(&self, id: &EngineTaskId, url: &str) -> Result<(), EngineError> {
        self.core
            .add_url_seed(id, url)
            .map_err(|e| EngineError::Other(core_err(&e)))
    }

    async fn ban_peer(&self, _id: &EngineTaskId, _peer: SocketAddr) -> Result<(), EngineError> {
        Err(EngineError::Unsupported)
    }

    async fn read_piece(&self, id: &EngineTaskId, idx: u32) -> Result<Vec<u8>, EngineError> {
        self.core
            .read_piece(id, idx as i32)
            .map(|o| o.unwrap_or_default())
            .map_err(|e| EngineError::Other(core_err(&e)))
    }
}
