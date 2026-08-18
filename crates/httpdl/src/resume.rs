//! .part 续传决策（§14 ETag 策略）：
//! ETag 一致 → 续传；ETag 不一致但服务器尊重 Range（试探 206）→ 续传；
//! 忽略 Range(200)/416/Length 变化 → 重下。
//! 附带 .part 的 ETag 持久化（`<part>.etag` 副文件，UTF-8 文本）。

use crate::range::Probe;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResumeDecision {
    /// 从偏移继续（偏移 = 现有 .part 长度）。
    ContinueFrom(u64),
    /// 作废重下（旧 .part 不可信）。
    Restart,
}

/// 依据 .part 现状与探测结果决定续传/重下。
/// - part 长度超过文件总长（Length 变化）→ 重下
/// - ETag 一致 → 续传
/// - 服务器尊重 Range（探测 206）→ 试探性续传（信任 Range 语义）
/// - 否则（200/416）→ 重下
pub fn decide_resume(part_len: u64, part_etag: Option<&str>, probe: &Probe) -> ResumeDecision {
    if probe.total.is_some_and(|t| part_len > t) {
        return ResumeDecision::Restart;
    }
    if part_etag.is_some() && part_etag == probe.etag.as_deref() {
        return ResumeDecision::ContinueFrom(part_len);
    }
    if probe.range_supported {
        ResumeDecision::ContinueFrom(part_len)
    } else {
        ResumeDecision::Restart
    }
}

/// .part 旁 ETag 副文件路径：`<part>.etag`。
pub fn part_etag_path(part: &Path) -> PathBuf {
    let mut s = part.as_os_str().to_os_string();
    s.push(".etag");
    PathBuf::from(s)
}

/// 读取 .part 的持久化 ETag（无副文件 → None）。
pub fn read_part_etag(part: &Path) -> Option<String> {
    std::fs::read_to_string(part_etag_path(part))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 持久化 .part 的 ETag；`etag=None` → 删除副文件。
pub fn write_part_etag(part: &Path, etag: Option<&str>) {
    let p = part_etag_path(part);
    match etag {
        Some(e) if !e.is_empty() => {
            let _ = std::fs::write(&p, e);
        }
        _ => {
            let _ = std::fs::remove_file(&p);
        }
    }
}
