"""
PoC: 迅雷 .xlbt.cfg 解析器 (真实样本验证版)

基于 2026-08-17 真实样本验证结果 (见 docs/research/xunlei/spec_pending_validation.md):
  .xlbt.cfg 真实格式 (A 级, 样本 C5AA149AE0776344A270EAFEE49FDADB43FF6097):
    +0x00 (8B):  magic = "XDLCTX\\x00\\x00"   (注意: 非旧推测的 "XLBTCFG")
    +0x08 (16B): 任务随机标识 (opaque, 语义未知)
    +0x18 (4B):  u32 = 30025 (语义未知, 与 piece 数相关候选)
    +0x1c (4B):  u32 = 7      (语义未知)
    +0x24 (4B):  u32 = 4      (语义未知)
    +0x2c (4B):  u32 = 28584  (语义未知)
    +0x30 (4B):  u32 = 4      (语义未知)
    +0x34 (4B):  u32 = 262145 (语义未知)
    +0x38 (4B):  u32 = 40     (infohash 字符串长度)
    +0x3c (40B): infohash ASCII 大写 hex (= torrent v1 info_hash)
    +0x64 起:    tag-02 int 记录表 [02 00 <key16> <val32>]:
                   key=1 → 已下载 piece 数 (样本: 1868, 与 xltd SHA1 验证 1864+12 部分吻合)
                   key=2.. → 0 (其余保留)
    文件内嵌:    u64 文件大小 (样本: 740642 cover.jpg @0x6f7c, 295849204 m4b @0x499c)
    peer 缓存:   "bt://<ip>:<port>" 字符串记录 (样本 8+ 条)
    blob 记录:   tag-04 [04 00 <len32> <data>]; 含 231 个 20B 随机 blob (非 torrent piece 哈希!)
    "Reserved"  8B 标签字段 ×4

  关键否定结论 (A 级):
    - cfg 中**没有** piece 哈希表 (2263×20=45KB > 32KB cfg, 且 231 个 20B blob 无一匹配 torrent pieces)
    - cfg 中**没有** bitfield (下载状态由 .bt.xltd 的零区表达, 见 validate_xunlei_sample.py V5/V6)

用法:
  python parse_xlbt_cfg.py <.xlbt.cfg 文件>
"""
import re
import struct
import json
import sys
from pathlib import Path

MAGIC_REAL = b"XDLCTX\x00\x00"
INFO_HASH_OFF = 0x3C
INT_TAG = b"\x02\x00"
BLOB_TAG = b"\x04\x00"


def parse_xlbt_cfg(path):
    """解析 .xlbt.cfg (真实格式, 保守: 只输出可证明的字段)"""
    data = Path(path).read_bytes()
    size = len(data)
    print(f"[*] parsing {path} ({size} bytes)")

    if size < 0x64:
        print(f"[ERR] too small: {size} bytes (need at least 100)")
        return None

    magic = data[0:8]
    magic_ok = magic == MAGIC_REAL
    print(f"\n=== 头部 ===")
    print(f"  magic (0x00, 8B):  {magic!r}  ({'OK' if magic_ok else 'MISMATCH!'})")
    print(f"  随机区 (0x08,16B): {data[8:24].hex()}")
    for off in (0x18, 0x1C, 0x24, 0x2C, 0x30, 0x34, 0x38):
        v = struct.unpack("<I", data[off:off + 4])[0]
        print(f"  u32@{off:#06x}: {v}")

    ih = data[INFO_HASH_OFF:INFO_HASH_OFF + 40].decode(errors="replace")
    print(f"  infohash (0x3c,40B): {ih}")

    # tag-02 int 记录表: 0x64 起, 8B/entry, [02 00 key16 val32]
    ints = []
    i = 0x64
    while i + 8 <= size and data[i:i + 2] == INT_TAG:
        key = struct.unpack("<H", data[i + 2:i + 4])[0]
        val = struct.unpack("<I", data[i + 4:i + 8])[0]
        ints.append((key, val))
        i += 8
    nz = [(k, v) for k, v in ints if v != 0]
    print(f"\n=== tag-02 int 记录 (共 {len(ints)} 条, 非零 {len(nz)} 条) ===")
    for k, v in nz[:10]:
        print(f"  key={k} → {v}")

    # tag-04 blob 记录统计
    blobs = []
    j = 0
    while j < size - 6:
        if data[j:j + 2] == BLOB_TAG:
            ln = struct.unpack("<I", data[j + 2:j + 6])[0]
            if j + 6 + ln <= size:
                blobs.append((j, ln, data[j + 6:j + 6 + ln]))
                j += 6 + ln
                continue
        j += 1
    print(f"\n=== tag-04 blob 记录: {len(blobs)} 条 ===")
    for off, ln, b in blobs[:8]:
        printable = b if all(32 <= x < 127 for x in b[:24]) else b[:12].hex()
        print(f"  @0x{off:04x} len={ln}: {printable!r}")

    # peer 缓存
    peers = [(m.start(), m.group().decode()) for m in re.finditer(rb"bt://[\d.]+:\d+", data)]
    print(f"\n=== peer 缓存: {len(peers)} 条 ===")
    for off, p in peers[:12]:
        print(f"  @0x{off:04x}: {p}")

    # 文件大小 u64 (已知文件大小可在此出现; 只列与 4096 对齐无关的明显值)
    print(f"\n[OK] 解析完成 (未解释区域: 0x{INFO_HASH_OFF + 40:#06x} 之后大部分 TLV 细节)")
    return {
        "size": size, "magic": magic.decode(errors="replace"), "magic_ok": magic_ok,
        "infohash": ih, "int_records": ints, "blob_count": len(blobs),
        "peers": [p for _, p in peers],
    }


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    r = parse_xlbt_cfg(sys.argv[1])
    if r:
        Path("samples/parse_report.json").write_text(
            json.dumps(r, indent=2, ensure_ascii=False), encoding="utf-8")
        print("\n[OK] report saved: samples/parse_report.json")
