//! 静态分块规划（§14 D11/D25）：
//! `N = clamp(file_size/64MB, 2, 8)`；段不相交 → 无文件锁；不支持 Range → 单连接流式。

/// 段（闭区间 [start, end]，长度 = end - start + 1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
}

const SEGMENT_SIZE: u64 = 64 * 1024 * 1024;
const MIN_SEGMENTS: u64 = 2;
const MAX_SEGMENTS: u64 = 8;

/// 段数（D11/D25 公式）。
pub fn segment_count(total: u64) -> usize {
    let n = (total / SEGMENT_SIZE).clamp(MIN_SEGMENTS, MAX_SEGMENTS);
    n as usize
}

/// 等分规划：前 `total % n` 段各多 1 字节，段连续覆盖 [0, total)。
pub fn plan_segments(total: u64) -> Vec<Segment> {
    if total == 0 {
        return vec![];
    }
    let n = segment_count(total) as u64;
    let base = total / n;
    let rem = total % n;
    (0..n)
        .map(|i| {
            let start = i * base + i.min(rem);
            let len = base + u64::from(i < rem);
            Segment {
                start,
                end: start + len - 1,
            }
        })
        .collect()
}