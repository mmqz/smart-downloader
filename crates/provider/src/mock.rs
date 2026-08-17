//! MockProvider：内存实现（§13 RemoteProvider），可注入配额/backoff/认证/直链配置。

use crate::types::{
    link_expired, ProviderError, ProviderRuntime, ProviderStatus, ProviderTaskId,
    ResolvedRemoteFile,
};
use smart_dl_core::types::{Capability, DownloadSource};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct MockTask {
    status: ProviderStatus,
    /// status() 调用次数（自动推进 Queued→Downloading→Ready）。
    status_calls: u32,
    /// 第几次 submit（1=首次，≥2=resubmit）。
    submit_seq: u32,
}

struct MockState {
    enabled: bool,
    authenticated: bool,
    quota_remaining: u64,
    concurrency_limit: u32,
    busy: u32,
    backoff_until: Option<u64>,
    last_error: Option<String>,
    /// 首次 submit 的 resolve 文件。
    files: Vec<ResolvedRemoteFile>,
    /// resubmit（第 ≥2 次 submit）的 resolve 文件。
    resubmit_files: Vec<ResolvedRemoteFile>,
    /// refresh_links 提供的更新 URL → resolve 时替换 files 的 url。
    update_urls: Option<Vec<String>>,
    /// refresh_links 是否已调用（update_urls 只在刷新后生效）。
    refreshed: bool,
    /// 下一次 submit 直接 Failed（fail_next_submits 注入）。
    fail_next: bool,
    next_id: u64,
    tasks: HashMap<ProviderTaskId, MockTask>,
}

/// 内存 Mock Provider：submit 后状态自动推进，resolve 返回配置的直链。
#[derive(Clone)]
pub struct MockProvider {
    name: String,
    state: Arc<Mutex<MockState>>,
}

impl MockProvider {
    pub fn new(name: &str) -> Self {
        MockProvider {
            name: name.to_string(),
            state: Arc::new(Mutex::new(MockState {
                enabled: true,
                authenticated: true,
                quota_remaining: u64::MAX,
                concurrency_limit: 2,
                busy: 0,
                backoff_until: None,
                last_error: None,
                files: vec![],
                resubmit_files: vec![],
                update_urls: None,
                refreshed: false,
                fail_next: false,
                next_id: 1,
                tasks: HashMap::new(),
            })),
        }
    }

    pub fn with_quota(self, q: u64) -> Self {
        self.state.lock().unwrap().quota_remaining = q;
        self
    }

    pub fn disabled(self) -> Self {
        self.state.lock().unwrap().enabled = false;
        self
    }

    pub fn unauthenticated(self) -> Self {
        self.state.lock().unwrap().authenticated = false;
        self
    }

    pub fn with_backoff(self, until_unix: u64) -> Self {
        self.state.lock().unwrap().backoff_until = Some(until_unix);
        self
    }

    pub fn with_concurrency(self, n: u32) -> Self {
        self.state.lock().unwrap().concurrency_limit = n;
        self
    }

    pub fn with_files(self, files: Vec<ResolvedRemoteFile>) -> Self {
        self.state.lock().unwrap().files = files;
        self
    }

    /// resubmit 轮次的 resolve 文件（新直链）。
    pub fn set_resubmit_files(&self, files: Vec<ResolvedRemoteFile>) {
        self.state.lock().unwrap().resubmit_files = files;
    }

    /// update_sources 携带的新 URL（refresh_links 输出，resolve 时替换 files url）。
    pub fn set_update_urls(&self, urls: Vec<String>) {
        self.state.lock().unwrap().update_urls = Some(urls);
    }

    /// 测试观察：当前占用并发。
    pub fn set_busy(&self, n: u32) {
        self.state.lock().unwrap().busy = n;
    }

    /// 测试注入：下一次 submit 创建的任务直接进入 Failed（poll_ready 失败分支）。
    pub fn fail_next_submits(&self) {
        let mut st = self.state.lock().unwrap();
        st.fail_next = true;
    }

    /// 测试观察：剩余配额。
    pub fn quota(&self) -> u64 {
        self.state.lock().unwrap().quota_remaining
    }

    /// refresh_links：update_sources 用的新 URL 列表（None = 无新链接）。
    pub fn refresh_links_sync(
        &self,
        id: &ProviderTaskId,
    ) -> Result<Option<Vec<String>>, ProviderError> {
        let mut st = self.state.lock().unwrap();
        if !st.tasks.contains_key(id) {
            return Err(ProviderError::NotFound);
        }
        st.refreshed = true;
        Ok(st.update_urls.clone())
    }
}

#[async_trait::async_trait]
impl crate::RemoteProvider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::OfflineCache]
    }

    fn runtime(&self) -> ProviderRuntime {
        let st = self.state.lock().unwrap();
        ProviderRuntime {
            enabled: st.enabled,
            authenticated: st.authenticated,
            quota_remaining: st.quota_remaining,
            concurrency_limit: st.concurrency_limit,
            busy: st.busy,
            backoff_until: st.backoff_until,
            last_error: st.last_error.clone(),
        }
    }

    async fn refresh_auth(&self) -> Result<(), ProviderError> {
        let mut st = self.state.lock().unwrap();
        st.authenticated = true;
        Ok(())
    }

    async fn submit(&self, _source: &DownloadSource) -> Result<ProviderTaskId, ProviderError> {
        let mut st = self.state.lock().unwrap();
        if !st.enabled {
            return Err(ProviderError::Other("disabled".into()));
        }
        if !st.authenticated {
            return Err(ProviderError::Auth);
        }
        if st.quota_remaining == 0 {
            return Err(ProviderError::Quota);
        }
        let id = format!("{}-{}", self.name, st.next_id);
        let seq = st.next_id as u32;
        st.next_id += 1;
        let initial = if st.fail_next {
            st.fail_next = false;
            ProviderStatus::Failed
        } else {
            ProviderStatus::Queued
        };
        st.tasks.insert(
            id.clone(),
            MockTask {
                status: initial,
                status_calls: 0,
                submit_seq: seq,
            },
        );
        Ok(id)
    }

    async fn status(&self, id: &ProviderTaskId) -> Result<ProviderStatus, ProviderError> {
        let mut st = self.state.lock().unwrap();
        let t = st.tasks.get_mut(id).ok_or(ProviderError::NotFound)?;
        t.status_calls += 1;
        // Failed 是终态（fail_task 注入）——不自动推进
        if t.status == ProviderStatus::Failed {
            return Ok(ProviderStatus::Failed);
        }
        t.status = match t.status_calls {
            1 => ProviderStatus::Queued,
            2 => ProviderStatus::Downloading,
            _ => ProviderStatus::Ready,
        };
        Ok(t.status)
    }

    async fn resolve(&self, id: &ProviderTaskId) -> Result<Vec<ResolvedRemoteFile>, ProviderError> {
        let st = self.state.lock().unwrap();
        let t = st.tasks.get(id).ok_or(ProviderError::NotFound)?;
        if t.status != ProviderStatus::Ready {
            return Err(ProviderError::Other("task not ready".into()));
        }
        // 首次 submit → files；resubmit → resubmit_files（后者优先）
        let base = if t.submit_seq >= 2 && !st.resubmit_files.is_empty() {
            &st.resubmit_files
        } else {
            &st.files
        };
        // update_urls → 替换 url（refresh 后的新直链，全新有效期）
        let out: Vec<ResolvedRemoteFile> = match &st.update_urls {
            Some(urls) if !urls.is_empty() && st.refreshed => base
                .iter()
                .enumerate()
                .map(|(i, f)| ResolvedRemoteFile {
                    url: urls.get(i).cloned().unwrap_or_else(|| f.url.clone()),
                    expires_at: None, // 刷新即续期
                    ..f.clone()
                })
                .collect(),
            _ => base.clone(),
        };
        Ok(out)
    }

    async fn remove(&self, id: &ProviderTaskId) -> Result<(), ProviderError> {
        let mut st = self.state.lock().unwrap();
        st.tasks.remove(id).ok_or(ProviderError::NotFound)?;
        Ok(())
    }

    async fn refresh_links(
        &self,
        id: &ProviderTaskId,
    ) -> Result<Option<Vec<String>>, ProviderError> {
        self.refresh_links_sync(id)
    }
}

/// 供 coordinator 区分"直链是否失效"的便捷判定（mock 也可用）。
pub(crate) fn any_expired(files: &[ResolvedRemoteFile], now: u64) -> bool {
    files.iter().any(|f| link_expired(f, now))
}
