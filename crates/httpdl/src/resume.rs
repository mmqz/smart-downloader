//! .part 续传决策（§14 ETag 策略）：
//! ETag 一致 → 续传；ETag 不一致但服务器尊重 Range（试探 206）→ 续传；
//! 忽略 Range(200)/416/Length 变化 → 重下。

use crate::range::Probe;

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
