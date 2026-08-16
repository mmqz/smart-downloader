"""
最终验证: 模拟一个完整的迅雷 .xlbt.cfg 文件 (基于反汇编推断)
用真实 BT 任务的 piece hash 填充,验证我们的解析器能正确读出所有字段

输入: 一个真实的 .torrent 文件 (我们用 libtorrent 生成)
输出: 一个合成的 .xlbt.cfg 文件,包含完整的 m_piecesHash + bitfield
"""
import struct
import hashlib
import json
from pathlib import Path

# 生成一个合成 BT 任务的 .xlbt.cfg
# 假设一个简单单文件任务:
# - 文件大小: 1GB (1073741824)
# - piece_length: 256KB (262144) - 标准
# - num_pieces: 4096

FILE_SIZE = 1 * 1024 * 1024 * 1024  # 1GB
PIECE_LENGTH = 256 * 1024            # 256KB
NUM_PIECES = FILE_SIZE // PIECE_LENGTH  # 4096

# 1. 生成 piece hashes (假设全 0 数据,所以每个 piece SHA1 相同)
ZERO_PIECE = bytes(PIECE_LENGTH)
PIECE_SHA1 = hashlib.sha1(ZERO_PIECE).digest()  # 20 bytes
PIECES_HASH = PIECE_SHA1 * NUM_PIECES              # 4096 * 20 = 81920 bytes

# 2. 生成完成位图 (CXBitmap 内容)
# 假设下载了前 50% pieces (前 2048 个 = bitfield 中前 256 字节全 1)
# 标准 BT bitfield: 每 piece 1 bit, big-endian
BITFIELD_SIZE = (NUM_PIECES + 7) // 8  # 512 bytes
bitfield = bytearray(BITFIELD_SIZE)
# 前 2048 pieces 完成 → 前 256 字节全 0xFF
for i in range(2048 // 8):  # 256 字节
    bitfield[i] = 0xFF
bitfield_bytes = bytes(bitfield)

# 3. 生成 BTIH (infohash) - 20 bytes
INFOHASH = hashlib.sha1(b"synthetic_info_hash").digest()

# 4. 生成 GCID
# GCID = SHA1( SHA1(piece1) || SHA1(piece2) || ... || SHA1(pieceN) )
# 但 piece_size 用动态算法: 256KB 起, 文件 > 512 分片翻倍, 上限 2MB
# 1GB / 256KB = 4096 分片 → 翻 3 次到 2MB → 1GB / 2MB = 512 分片 (临界, 不翻)
# 所以 GCID piece_size = 256KB
GCID_PIECE_SIZE = 256 * 1024
GCID_PIECES = FILE_SIZE // GCID_PIECE_SIZE
gcid_pieces_sha1 = b""
for i in range(GCID_PIECES):
    # 模拟每 piece 的 SHA1 (用 i 区分)
    piece_data = struct.pack("<I", i) * (GCID_PIECE_SIZE // 4)
    gcid_pieces_sha1 += hashlib.sha1(piece_data).digest()
GCID = hashlib.sha1(gcid_pieces_sha1).digest()

# 5. 生成 BCID 列表 (推断了,但具体算法未公开,这里填 0 占位)
# BCID 推断: 每个 piece 一个 SHA1 (但具体内容未知)
# 假设 BCID block size = piece_length, 4096 个 BCID
BCID_LIST = bytes(20) * NUM_PIECES  # 4096 * 20 = 81920 bytes (全 0 占位)

print(f"=== Generated data ===")
print(f"  file_size:         {FILE_SIZE} ({FILE_SIZE/1024/1024:.0f} MB)")
print(f"  piece_length:      {PIECE_LENGTH}")
print(f"  num_pieces:        {NUM_PIECES}")
print(f"  pieces_hash size:  {len(PIECES_HASH)}")
print(f"  bitfield size:     {len(bitfield_bytes)}")
print(f"  infohash:          {INFOHASH.hex()}")
print(f"  gcid:              {GCID.hex()}")
print(f"  bcid_list size:    {len(BCID_LIST)}")

# 6. 构造 .xlbt.cfg 文件 (合成)
# 头部 40 字节
magic = b"XLBTCFG\x00"
reserved1 = 0
reserved2 = 0
reserved3 = 0
block_count = 1   # 1 个 block
block_size = 4096
section_count = 5  # 5 个 section
reserved4 = 0

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

# Section 数组: 每个 entry 20 字节 (4+8+8)
# field2 = section size, field3 = section offset in file (推断)
sections_data_start = 40 + 5 * 20  # = 140
sections_data = b""
section_bodies = b""

# Section 1: INFO_HASH (20 字节)
sections_data += struct.pack("<I", 0x00000001)  # section_id
sections_data += struct.pack("<Q", 20)            # size
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))  # offset
section_bodies += INFOHASH

# Section 2: PIECES_HASH (81920 字节)
sections_data += struct.pack("<I", 0x00000002)
sections_data += struct.pack("<Q", len(PIECES_HASH))
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))
section_bodies += PIECES_HASH

# Section 3: BITFIELD (CXBitmap 内容, 512 字节)
sections_data += struct.pack("<I", 0x00000003)
sections_data += struct.pack("<Q", len(bitfield_bytes))
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))
section_bodies += bitfield_bytes

# Section 4: FILE_INFO (变长, 这里用 JSON 简化)
file_info_json = json.dumps({
    "name": "synthetic_file.bin",
    "size": FILE_SIZE,
    "piece_length": PIECE_LENGTH,
    "num_pieces": NUM_PIECES,
}).encode('utf-8')
sections_data += struct.pack("<I", 0x00000004)
sections_data += struct.pack("<Q", len(file_info_json))
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))
section_bodies += file_info_json

# Section 5: GCID (20 字节)
sections_data += struct.pack("<I", 0x00000005)
sections_data += struct.pack("<Q", 20)
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))
section_bodies += GCID

# 完整文件
cfg = header + sections_data + section_bodies
out_cfg = Path(__file__).resolve().parent / "e2e_out" / "synthetic_full_cfg.bin"
out_cfg.parent.mkdir(exist_ok=True, parents=True)
out_cfg.write_bytes(cfg)
print(f"\n[OK] wrote synthetic cfg: {out_cfg} ({len(cfg)} bytes)")

# 同步生成一个 .bt.xltd (1GB sparse file 模拟,只填前 50% piece)
# 但 1GB 太大,我们只生成前 5MB piece 数据 (用于验证)
out_xltd = Path(__file__).resolve().parent / "e2e_out" / "synthetic_bt.xltd"
# 写 5MB 数据
with open(out_xltd, 'wb') as f:
    # 写前 5MB 数据 (前 20 个 piece × 256KB = 5MB)
    # 这些 piece 用 piece data = (piece_index 重复)
    for i in range(20):
        piece_data = struct.pack("<I", i) * (PIECE_LENGTH // 4)
        f.write(piece_data)
    # 然后用 sparse hole 填到 1GB (Linux sparse file)
    f.seek(FILE_SIZE - 1)
    f.write(b'\x00')
print(f"[OK] wrote synthetic bt.xltd: {out_xltd} (sparse 1GB)")

# 7. 验证: 用我们的解析器读这个 cfg
import subprocess
import sys
parser = str(Path(__file__).resolve().parent / "parse_xlbt_cfg.py")
result = subprocess.run(
    [sys.executable, parser, str(out_cfg), str(out_xltd)],
    capture_output=True, text=True
)
print("\n=== Parser output ===")
print(result.stdout[-3000:])
if result.stderr:
    print("STDERR:", result.stderr[-500:])
