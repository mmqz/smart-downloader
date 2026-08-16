"""
测试用: 生成符合 .xlbt.cfg 反汇编推断格式的合成文件
用来验证 parse_xlbt_cfg.py 是否正确解析
"""
import struct
import json
import subprocess
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent / "e2e_out" / "synthetic_cfg.bin"
OUT.parent.mkdir(exist_ok=True, parents=True)

# 头部 40 字节
# 模拟一个 BT 任务 cfg
magic = b"XLBTCFG\x00"           # +0x00 (8B)
reserved1 = 0                     # +0x08 (2B)
reserved2 = 0                     # +0x0A (2B)
reserved3 = 0                     # +0x0C (4B)
block_count = 256                 # +0x10 (8B) - 假设 256 blocks
block_size = 4096                 # +0x18 (8B) - 必须 4096 倍数
section_count = 5                 # +0x20 (4B)
reserved4 = 0                     # +0x24 (4B)

header = (
    magic +
    struct.pack("<H", reserved1) +
    struct.pack("<H", reserved2) +
    struct.pack("<I", reserved3) +
    struct.pack("<Q", block_count) +
    struct.pack("<Q", block_size) +
    struct.pack("<I", section_count) +
    struct.pack("<I", reserved4)
)
assert len(header) == 40, f"header size {len(header)} != 40"

# Section 数组,每 entry 20 字节
# 假设的 section_id 列表 (从反汇编推断)
sections = [
    # (section_id, field2=offset/size, field3=reserved)
    (0x00000001, 0x1000, 0),    # SECTION_INFO_HASH ?
    (0x00000002, 0x2000, 0),    # SECTION_PIECES_HASH ?
    (0x00000003, 0x3000, 0),    # SECTION_BITFIELD ?
    (0x00000004, 0x4000, 0),    # SECTION_FILE_INFO ?
    (0x00000005, 0x5000, 0),    # SECTION_BCID ?
]
section_data = b""
for sid, f2, f3 in sections:
    section_data += struct.pack("<I", sid)
    section_data += struct.pack("<Q", f2)
    section_data += struct.pack("<Q", f3)
assert len(section_data) == 5 * 20

# 完整 cfg 文件
cfg = header + section_data
OUT.write_bytes(cfg)
print(f"[OK] wrote synthetic cfg: {OUT} ({len(cfg)} bytes)")

# 用 parser 测试
parser = str(Path(__file__).resolve().parent / "parse_xlbt_cfg.py")
result = subprocess.run(
    [sys.executable, parser, str(OUT)],
    capture_output=True, text=True
)
print(result.stdout)
if result.stderr:
    print("STDERR:", result.stderr)
