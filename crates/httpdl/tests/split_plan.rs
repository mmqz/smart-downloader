//! M4a: 静态分块规划（§14 D11/D25）。
//! N = clamp(file_size/64MB, 2, 8)；段不相交且覆盖全文件。

use smart_dl_httpdl::static_split::{plan_segments, plan_segments_from, segment_count};

const MB: u64 = 1024 * 1024;

#[test]
fn plan_100mb_is_2_segments() {
    // 100MB/64MB = 1.56 → floor 1 → clamp 2
    assert_eq!(segment_count(100 * MB), 2);
    assert_eq!(plan_segments(100 * MB).len(), 2);
}

#[test]
fn plan_1gb_is_8_segments() {
    // 1024MB/64MB = 16 → clamp 8。
    // 注：计划文档 §M4 写"1GB→4"，与设计文档 §14 公式 N=clamp(size/64MB,2,8) 冲突；公式为准。
    assert_eq!(segment_count(1024 * MB), 8);
    assert_eq!(plan_segments(1024 * MB).len(), 8);
}

#[test]
fn plan_10gb_capped_at_8() {
    assert_eq!(segment_count(10 * 1024 * MB), 8);
}

#[test]
fn plan_small_file_floor_is_2() {
    // 10MB < 64MB → floor 0 → clamp 下限 2
    assert_eq!(segment_count(10 * MB), 2);
    assert_eq!(plan_segments(10 * MB).len(), 2);
}

#[test]
fn segments_cover_file_exactly() {
    // 不相交 + 覆盖全文件 + 连续
    for size in [
        1u64,
        10 * MB,
        100 * MB,
        1024 * MB,
        10 * 1024 * MB,
        7 * 1024 * 1024 * MB,
    ] {
        let segs = plan_segments(size);
        assert_eq!(segs.first().unwrap().start, 0);
        assert_eq!(segs.last().unwrap().end, size - 1);
        for w in segs.windows(2) {
            assert_eq!(w[0].end + 1, w[1].start, "段必须连续（无重叠无空洞）");
        }
    }
}

#[test]
fn zero_size_plan_is_empty() {
    assert!(plan_segments(0).is_empty());
}

// ---- #4 续传规划：plan_segments_from（只覆盖 [offset, total)）----

#[test]
fn resume_plan_covers_remaining_span() {
    // 100MB 文件，40MB 已下 → 段必须覆盖 [40MB, 100MB) 且连续无重叠
    let offset = 40 * MB;
    let total = 100 * MB;
    let segs = plan_segments_from(offset, total);
    assert!(!segs.is_empty());
    assert_eq!(segs.first().unwrap().start, offset, "首段从偏移开始");
    assert_eq!(segs.last().unwrap().end, total - 1, "末段到文件尾");
    for w in segs.windows(2) {
        assert_eq!(w[0].end + 1, w[1].start, "续传段必须连续");
    }
    // 总覆盖长度 = 剩余字节
    let covered: u64 = segs.iter().map(|s| s.len()).sum();
    assert_eq!(covered, total - offset);
}

#[test]
fn resume_plan_at_zero_equals_full_plan() {
    // offset=0 → 与全量规划一致（覆盖 [0, total)）
    for size in [10 * MB, 100 * MB, 1024 * MB] {
        assert_eq!(plan_segments_from(0, size), plan_segments(size));
    }
}

#[test]
fn resume_plan_offset_near_end_is_small() {
    // 偏移接近末尾 → 剩余覆盖正确（无越界）
    let total = 100 * MB;
    let offset = total - 10;
    let segs = plan_segments_from(offset, total);
    let covered: u64 = segs.iter().map(|s| s.len()).sum();
    assert_eq!(covered, 10);
    assert_eq!(segs.first().unwrap().start, offset);
    assert_eq!(segs.last().unwrap().end, total - 1);
}

#[test]
fn resume_plan_offset_at_or_beyond_total_is_empty() {
    assert!(
        plan_segments_from(100 * MB, 100 * MB).is_empty(),
        "偏移==总长 → 无段"
    );
    assert!(
        plan_segments_from(101 * MB, 100 * MB).is_empty(),
        "偏移>总长 → 无段"
    );
    assert!(plan_segments_from(0, 0).is_empty());
}
