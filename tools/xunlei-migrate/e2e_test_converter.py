"""
端到端测试: 用 libtorrent 生成一个最小 .torrent + 合成 .bt.xltd + 合成 .xlbt.cfg
然后跑转换器的诊断模式,验证整套流程能跑通

模拟一个真实 BT 任务场景:
- .torrent: 用 libtorrent 生成 (含真实 infohash + piece hashes)
- .bt.xltd: sparse file,只填前 N 个 piece 数据 (用对应 hash 的数据)
- .xlbt.cfg: 用我们的 spec 格式生成 (含 magic + section 数组 + 假设的 section 内容)
"""
import hashlib
import json
import struct
import os
import sys
from pathlib import Path

import libtorrent as lt

# stdout/stderr 统一 UTF-8,避免 Windows GBK 控制台乱码
if sys.stdout and hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

# 输出目录固定在脚本所在目录下,与仓库自包含
OUT = Path(__file__).resolve().parent / "e2e_out"
OUT.mkdir(exist_ok=True, parents=True)

# === 1. 生成一个最小 .torrent ===
# 文件大小: 1MB, piece_length: 256KB → 4 pieces
FILE_SIZE = 1 * 1024 * 1024
PIECE_LENGTH = 256 * 1024
NUM_PIECES = FILE_SIZE // PIECE_LENGTH  # 4

# 生成"原始文件" - 用 piece index 作为内容区分
file_path = OUT / "source_file.bin"
with open(file_path, 'wb') as f:
    for i in range(NUM_PIECES):
        # 每 piece 256KB,内容是 piece_index 重复
        f.write(struct.pack("<I", i) * (PIECE_LENGTH // 4))

print(f"[1] 生成源文件: {file_path} ({FILE_SIZE} bytes)")

# 用 libtorrent 创建 .torrent
fs = lt.file_storage()
# 以文件名为 torrent 内路径 (单文件种子),避免 Windows 绝对路径解析问题
fs.add_file("source_file.bin", FILE_SIZE)
fs.set_piece_length(PIECE_LENGTH)

t = lt.create_torrent(fs)
# 计算 piece hashes - 用 set_piece_hashes 自动计算
lt.set_piece_hashes(t, str(file_path.parent))

# 添加一些 tracker
t.add_tracker("http://tracker.opentrackr.org:1337/announce")

# 生成 .torrent 文件
torrent_data = lt.bencode(t.generate())
torrent_path = OUT / "test.torrent"
torrent_path.write_bytes(torrent_data)
print(f"[2] 生成 .torrent: {torrent_path}")

# === 2. 生成 .bt.xltd (sparse file,只填前 50% piece) ===
# 模拟"下载到 50%"
# 注意: libtorrent 可能用 16KB piece_length 而不是我们设的 256KB
# 所以这里用 set_piece_hashes 之后的 piece_length
# 先生成 .torrent 拿 piece_length, 然后用相同 piece_length 生成 .bt.xltd

bt_xltd_path = OUT / "test.bt.xltd"

# 真实 piece_length 在生成 .torrent 后才知道, 后面会重新生成 .bt.xltd
# 先占位,等拿到 piece_length 后再生成
print(f"[3] (delayed) .bt.xltd will be generated after .torrent")

# === 3. 生成 .xlbt.cfg ===
# 拿真实 info_hash + pieces_hash
info = lt.torrent_info(str(torrent_path))
info_hash = info.info_hash().to_bytes()  # sha1_hash → bytes
print(f"  info_hash (from torrent): {info_hash.hex()}")

# 从 .torrent 文件直接 bdecode 拿 pieces_hash (避开 libtorrent 不暴露的问题)
torrent_raw = torrent_path.read_bytes()
# 简单 bdecode
def bdecode(data, pos=0):
    c = chr(data[pos])
    if c == 'd':
        pos += 1; d = {}
        while data[pos] != ord('e'):
            k, pos = bdecode(data, pos)
            v, pos = bdecode(data, pos)
            d[k] = v
        return d, pos + 1
    elif c == 'l':
        pos += 1; l = []
        while data[pos] != ord('e'):
            v, pos = bdecode(data, pos)
            l.append(v)
        return l, pos + 1
    elif c == 'i':
        end = data.index(b'e', pos)
        return int(data[pos+1:end]), end + 1
    elif c.isdigit():
        colon = data.index(b':', pos)
        n = int(data[pos:colon])
        start = colon + 1
        return data[start:start+n], start + n

parsed, _ = bdecode(torrent_raw)
info_dict = parsed[b'info']
pieces_hash = info_dict[b'pieces']
print(f"  pieces_hash length: {len(pieces_hash)} bytes ({len(pieces_hash)//20} pieces)")
print(f"  piece_length from torrent: {info_dict[b'piece length']}")

# 现在有了真实 piece_length, 重新生成 .bt.xltd
real_piece_length = info_dict[b'piece length']
real_num_pieces = len(pieces_hash) // 20
# 模拟"下载到 50%": 前 real_num_pieces/2 个 piece 有数据
half_pieces = real_num_pieces // 2

with open(bt_xltd_path, 'wb') as f:
    # 前 half_pieces 个 piece 写真实数据 (从 source_file 读)
    with open(file_path, 'rb') as src:
        for i in range(half_pieces):
            src.seek(i * real_piece_length)
            piece_data = src.read(real_piece_length)
            f.write(piece_data)
    # 后半部分用 sparse hole
    f.seek(FILE_SIZE - 1)
    f.write(b'\x00')

# 检查 sparse (Windows 无 st_blocks, 仅 Linux 可测)
stat = bt_xltd_path.stat()
actual = getattr(stat, "st_blocks", 0) * 512
print(f"[3] 生成 .bt.xltd: {bt_xltd_path} (size={stat.st_size}, actual_blocks={actual})")

# bitfield: 从 piece_length 算 num_pieces
real_piece_length = info_dict[b'piece length']
real_num_pieces = (FILE_SIZE + real_piece_length - 1) // real_piece_length
# 模拟下载到 50%: 前 real_num_pieces/2 个完成
real_bitfield_size = (real_num_pieces + 7) // 8
bitfield = bytearray(real_bitfield_size)
for i in range(real_num_pieces // 2):
    byte_idx = i // 8
    bit_idx = 7 - (i % 8)  # big-endian (标准 BT)
    bitfield[byte_idx] |= (1 << bit_idx)
bitfield = bytes(bitfield)
print(f"  bitfield size: {len(bitfield)} bytes ({real_num_pieces} pieces)")

# 构造 .xlbt.cfg (按 spec_pending_validation.md 推测的格式)
header = b"XLBTCFG\x00"     # +0x00 magic
header += struct.pack("<H", 0)  # +0x08 reserved1
header += struct.pack("<H", 0)  # +0x0A reserved2
header += struct.pack("<I", 0)  # +0x0C reserved3
header += struct.pack("<Q", 1)  # +0x10 block_count
header += struct.pack("<Q", 4096)  # +0x18 block_size
header += struct.pack("<I", 5)   # +0x20 section_count
header += struct.pack("<I", 0)   # +0x24 reserved4
assert len(header) == 40

# Section 数组 (每 entry 20B = 4+8+8)
sections_data_start = 40 + 5 * 20  # 140
sections_data = b""
section_bodies = b""

# Section 1: INFO_HASH (20 字节)
sections_data += struct.pack("<I", 0x00000001)  # section_id (D 级猜测)
sections_data += struct.pack("<Q", 20)            # size
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))  # offset
section_bodies += info_hash

# Section 2: PIECES_HASH (4 × 20 = 80 字节)
sections_data += struct.pack("<I", 0x00000002)
sections_data += struct.pack("<Q", len(pieces_hash))
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))
section_bodies += pieces_hash

# Section 3: BITFIELD (1 字节, 4 pieces / 8 = 1 byte)
sections_data += struct.pack("<I", 0x00000003)
sections_data += struct.pack("<Q", len(bitfield))
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))
section_bodies += bitfield

# Section 4: FILE_INFO (JSON 简化)
file_info = json.dumps({"name": "source_file.bin", "size": FILE_SIZE}).encode('utf-8')
sections_data += struct.pack("<I", 0x00000004)
sections_data += struct.pack("<Q", len(file_info))
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))
section_bodies += file_info

# Section 5: GCID (20 字节)
gcid = hashlib.sha1(hashlib.sha1(struct.pack("<I", 0) * (PIECE_LENGTH // 4)).digest() * NUM_PIECES).digest()
sections_data += struct.pack("<I", 0x00000005)
sections_data += struct.pack("<Q", 20)
sections_data += struct.pack("<Q", sections_data_start + len(section_bodies))
section_bodies += gcid

cfg = header + sections_data + section_bodies
cfg_path = OUT / "test.xlbt.cfg"
cfg_path.write_bytes(cfg)
print(f"[4] 生成 .xlbt.cfg: {cfg_path} ({len(cfg)} bytes)")

# === 4. 运行转换器诊断模式 ===
print("\n" + "="*60)
print("[5] 运行转换器诊断模式")
print("="*60)
import subprocess
converter = str(Path(__file__).resolve().parent / "xunlei_to_libtorrent_converter.py")
result = subprocess.run(
    [sys.executable, converter,
     "--torrent", str(torrent_path),
     "--bt-xltd", str(bt_xltd_path),
     "--cfg", str(cfg_path),
     "--output-dir", str(OUT / "diagnostic_out")],
    capture_output=True, text=True
)
print(result.stdout[-3000:])
if result.stderr:
    print("STDERR:", result.stderr[-1000:])

# === 5. 运行转换器转换模式 ===
print("\n" + "="*60)
print("[6] 运行转换器转换模式")
print("="*60)
result = subprocess.run(
    [sys.executable, converter,
     "--torrent", str(torrent_path),
     "--bt-xltd", str(bt_xltd_path),
     "--cfg", str(cfg_path),
     "--output-dir", str(OUT / "convert_out"),
     "--convert"],
    capture_output=True, text=True
)
print(result.stdout[-3000:])
if result.stderr:
    print("STDERR:", result.stderr[-1000:])

# 列出输出
print("\n" + "="*60)
print("[7] 输出文件清单")
print("="*60)
for p in sorted(OUT.rglob("*")):
    if p.is_file():
        print(f"  {p.relative_to(OUT)}  ({p.stat().st_size} bytes)")
