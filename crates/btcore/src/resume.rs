//! resume 异步流（D16）：request_save_resume → RESUME alert(resume_ready) →
//! take_resume_data → ResumeBytes（bencode 数据，可落盘 / lt_add_torrent_resume 回灌）。

/// resume 数据（bencode；可直接写盘供下次启动 add_torrent_resume）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeBytes(pub Vec<u8>);

impl ResumeBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<u8>> for ResumeBytes {
    fn from(v: Vec<u8>) -> Self {
        ResumeBytes(v)
    }
}