//! HttpEngine（§14，impl DownloadEngine）：M4a 骨架。
//! add = 提取 URL → Range 探测 → 静态分块规划 → 登记任务；
//! M4b 补充：多连接并行下载、镜像、update_sources 换源、校验、限速。

use crate::range::probe_range;
use crate::static_split::plan_segments;
use smart_dl_core::task::DownloadTask;
use smart_dl_core::types::{
    Capability, DownloadEngine, DownloadSource, EngineError, EngineKind, EngineState, EngineStatus,
    EngineTaskId, PeerInfo,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;

/// 引擎内任务（M4a：探测 + 规划结果；M4b 挂下载状态）。
/// url/headers 为 M4b 换源/重试预留（M4a 骨架不读）。
#[allow(dead_code)]
struct HttpTask {
    url: String,
    headers: Vec<(String, String)>,
    state: EngineState,
    metadata_received: bool,
    total: u64,
    done: u64,
    error: Option<String>,
}

/// HTTP 引擎：reqwest 传输 + 自研调度层（D29）。
pub struct HttpEngine {
    client: reqwest::Client,
    tasks: Mutex<HashMap<EngineTaskId, HttpTask>>,
}

impl HttpEngine {
    pub fn new(client: reqwest::Client) -> Self {
        HttpEngine {
            client,
            tasks: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl DownloadEngine for HttpEngine {
    fn id(&self) -> &str {
        "http"
    }

    fn kind(&self) -> EngineKind {
        EngineKind::Http
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::Http,
            Capability::Https,
            Capability::Range,
            Capability::MultiConnection,
            Capability::Mirror,
            Capability::UrlRefresh,
            Capability::Sequential,
        ]
    }

    async fn add(&self, task: &DownloadTask) -> Result<EngineTaskId, EngineError> {
        let (url, headers, _auth) = match &task.source {
            DownloadSource::Http { url, headers, auth: _ } => {
                (url.clone(), headers.clone(), ())
            }
            _ => return Err(EngineError::Other("source is not http".to_string())),
        };
        let probe = probe_range(&self.client, &url, &headers).await?;
        let total = probe.total.unwrap_or(0);
        // 规划（M4b 用段表驱动多连接；M4a 仅记录总长）
        let _segments = plan_segments(total);

        let tid = task.id.clone();
        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(
            tid.clone(),
            HttpTask {
                url,
                headers,
                state: EngineState::Downloading,
                metadata_received: true,
                total,
                done: 0,
                error: None,
            },
        );
        Ok(tid)
    }

    async fn pause(&self, id: &EngineTaskId) -> Result<(), EngineError> {
        let mut tasks = self.tasks.lock().unwrap();
        let t = tasks.get_mut(id).ok_or(EngineError::NotFound)?;
        t.state = EngineState::Paused;
        Ok(())
    }

    async fn resume(&self, id: &EngineTaskId) -> Result<(), EngineError> {
        let mut tasks = self.tasks.lock().unwrap();
        let t = tasks.get_mut(id).ok_or(EngineError::NotFound)?;
        t.state = EngineState::Downloading;
        Ok(())
    }

    async fn status(&self, id: &EngineTaskId) -> Result<EngineStatus, EngineError> {
        let tasks = self.tasks.lock().unwrap();
        let t = tasks.get(id).ok_or(EngineError::NotFound)?;
        Ok(EngineStatus {
            state: t.state,
            metadata_received: t.metadata_received,
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
        let mut tasks = self.tasks.lock().unwrap();
        tasks.remove(id).ok_or(EngineError::NotFound)?;
        Ok(())
    }

    async fn peers(&self, _id: &EngineTaskId) -> Result<Vec<PeerInfo>, EngineError> {
        // HTTP 无 peer 概念（D3 反吸血仅 BT）
        Ok(vec![])
    }

    async fn update_sources(&self, _id: &EngineTaskId, _urls: Vec<String>) -> Result<(), EngineError> {
        // M4b 实现（换源）；M4a 骨架先接受
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