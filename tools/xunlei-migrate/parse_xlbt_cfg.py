"""
PoC: 迅雷 .xlbt.cfg 解析器 + .bt.xltd 探测器

基于反汇编推断的字段布局 (A 级证据):
  .xlbt.cfg 文件头 (40 字节 = 0x28):
    +0x00 (8B):  magic = "XLBTCFG\x00"
    +0x08 (2B):  reserved (0)
    +0x0A (2B):  ? (可能 reserved)
    +0x10 (8B):  block_count  (qword, little-endian)
    +0x18 (8B):  block_size   (qword, 必须 4096 倍数)
    +0x20 (4B):  section_count (dword)
  
  紧接着是 section 数组, 每 entry 20 字节:
    +0x00 (4B):  section_id (dword)
    +0x04 (8B):  offset 或 size (qword)
    +0x0C (8B):  reserved 或第二字段 (qword)

.bt.xltd 文件 (B 级推断):
  无 ASCII magic, 推断为纯 piece 数据 sparse file
  按 piece_index * piece_length 偏移存储

用法:
  python3 parse_xlbt_cfg.py <.xlbt.cfg 文件> [<.bt.xltd 文件>]
"""
import sys
import struct
import json
from pathlib import Path


def parse_xlbt_cfg(path):
    """解析 .xlbt.cfg 文件"""
    data = Path(path).read_bytes()
    size = len(data)
    print(f"[*] parsing {path} ({size} bytes)")
    
    if size < 0x28:
        print(f"[ERR] too small: {size} bytes (need at least 40)")
        return None
    
    # 头部 40 字节
    magic = data[0:8]
    reserved1 = struct.unpack("<H", data[8:10])[0]
    reserved2 = struct.unpack("<H", data[10:12])[0]
    reserved3 = struct.unpack("<I", data[12:16])[0]
    block_count = struct.unpack("<Q", data[16:24])[0]
    block_size = struct.unpack("<Q", data[24:32])[0]
    section_count = struct.unpack("<I", data[32:36])[0]
    reserved4 = struct.unpack("<I", data[36:40])[0]
    
    print(f"\n=== Header (40 bytes = 0x28) ===")
    EXPECTED_MAGIC = b"XLBTCFG\x00"
    print(f"  magic:          {magic!r}  ({'OK' if magic == EXPECTED_MAGIC else 'MISMATCH!'})")
    print(f"  reserved1 (0x08, 2B):   0x{reserved1:04x} ({reserved1})")
    print(f"  reserved2 (0x0A, 2B):   0x{reserved2:04x} ({reserved2})")
    print(f"  reserved3 (0x0C, 4B):   0x{reserved3:08x} ({reserved3})")
    print(f"  block_count (0x10, 8B): {block_count} (0x{block_count:x})")
    print(f"  block_size (0x18, 8B):  {block_size} (0x{block_size:x})  align4096={'OK' if block_size % 4096 == 0 else 'FAIL'}")
    print(f"  section_count (0x20, 4B): {section_count}")
    print(f"  reserved4 (0x24, 4B):  0x{reserved4:08x} ({reserved4})")
    
    # Hex dump 头部
    print(f"\n  header hex dump:")
    for i in range(0, 0x28, 16):
        line_hex = " ".join(f"{b:02x}" for b in data[i:i+16])
        line_ascii = "".join(chr(b) if 32 <= b < 127 else "." for b in data[i:i+16])
        print(f"    {i:04x}:  {line_hex}  |{line_ascii}|")
    
    # Section 数组
    sections = []
    print(f"\n=== Sections ({section_count} entries, each 20 bytes = 0x14) ===")
    for i in range(min(section_count, 100)):  # 防止过大
        offset = 0x28 + i * 0x14
        if offset + 0x14 > size:
            print(f"  [ERR] section {i} out of bounds (offset 0x{offset:x})")
            break
        section_id = struct.unpack("<I", data[offset:offset+4])[0]
        field2 = struct.unpack("<Q", data[offset+4:offset+12])[0]
        field3 = struct.unpack("<Q", data[offset+12:offset+20])[0]
        sections.append({
            "index": i,
            "offset_in_file": offset,
            "section_id": section_id,
            "field2": field2,
            "field3": field3,
        })
        print(f"  [{i}] @0x{offset:04x}:  section_id=0x{section_id:08x} ({section_id})  field2=0x{field2:016x} ({field2})  field3=0x{field3:016x} ({field3})")
    
    if section_count > 100:
        print(f"  ... ({section_count - 100} more)")
    
    return {
        "size": size,
        "magic": magic.decode('ascii', errors='replace'),
        "magic_ok": magic == b"XLBTCFG\x00",
        "reserved1": reserved1,
        "reserved2": reserved2,
        "reserved3": reserved3,
        "block_count": block_count,
        "block_size": block_size,
        "block_size_aligned": block_size % 4096 == 0,
        "section_count": section_count,
        "reserved4": reserved4,
        "sections": sections,
    }


def probe_bt_xltd(path):
    """探测 .bt.xltd 文件结构"""
    data = Path(path).read_bytes()
    size = len(data)
    print(f"\n[*] probing {path} ({size} bytes = {size/1024/1024:.2f} MB)")
    
    # 检查前 64 字节,看是否有 magic
    print(f"\n  first 64 bytes hex:")
    for i in range(0, min(64, size), 16):
        line_hex = " ".join(f"{b:02x}" for b in data[i:i+16])
        line_ascii = "".join(chr(b) if 32 <= b < 127 else "." for b in data[i:i+16])
        print(f"    {i:04x}:  {line_hex}  |{line_ascii}|")
    
    # 检查是否是 ASCII magic (前 8 字节全可打印)
    first8 = data[0:8]
    is_ascii_magic = all(32 <= b < 127 for b in first8) and first8 != b'\x00' * 8
    print(f"\n  first 8 bytes ASCII? {is_ascii_magic}")
    if is_ascii_magic:
        print(f"    magic = {first8!r}")
    
    # 检查是否是 sparse file (大量 0 字节)
    # 用 NTFS sparse 检测在 Linux 上无效,但我们能看文件实际占用 vs size
    
    # 检查末尾 64 字节
    if size > 64:
        print(f"\n  last 64 bytes hex:")
        for i in range(max(0, size - 64), size, 16):
            line_hex = " ".join(f"{b:02x}" for b in data[i:i+16])
            line_ascii = "".join(chr(b) if 32 <= b < 127 else "." for b in data[i:i+16])
            print(f"    {i:08x}:  {line_hex}  |{line_ascii}|")
    
    # 检查 0x00 区域比例 (sparse file 检测)
    # 采样检查 16 个位置,每位置 4KB
    if size > 65536:
        sample_positions = [size // 16 * i for i in range(16)]
        zero_blocks = 0
        non_zero_blocks = 0
        for pos in sample_positions:
            block = data[pos:pos+4096]
            if all(b == 0 for b in block):
                zero_blocks += 1
            else:
                non_zero_blocks += 1
        print(f"\n  sparse sampling (16 positions × 4KB):")
        print(f"    zero blocks:     {zero_blocks}")
        print(f"    non-zero blocks: {non_zero_blocks}")
        print(f"    sparse ratio:   {zero_blocks/16*100:.0f}%")
    
    return {
        "size": size,
        "first8_hex": first8.hex(),
        "is_ascii_magic": is_ascii_magic,
    }


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    
    cfg_path = sys.argv[1]
    cfg_info = parse_xlbt_cfg(cfg_path)
    
    xltd_info = None
    if len(sys.argv) >= 3:
        xltd_path = sys.argv[2]
        xltd_info = probe_bt_xltd(xltd_path)
    
    # 输出 JSON 报告 (写到输入 cfg 同目录)
    out = Path(sys.argv[1]).resolve().parent / "parse_report.json"
    out.write_text(json.dumps({
        "cfg": cfg_info,
        "xltd": xltd_info,
    }, indent=2, ensure_ascii=False, default=str))
    print(f"\n[OK] report saved: {out}")


if __name__ == "__main__":
    main()
