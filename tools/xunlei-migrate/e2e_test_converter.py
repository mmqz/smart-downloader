"""
端到端测试: 真实格式合成样本 → 转换器全流程

生成 (全部真实格式, 见 spec_pending_validation.md):
  - test.torrent:   libtorrent 生成 (真实 infohash + piece hashes)
  - test.bt.xltd:   4096 对齐位置镜像, 前 half_pieces 有数据 (零填充), 无头部
  - test.xlbt.cfg:  magic=XDLCTX\\0\\0 + 随机区 + 头部字段 + 0x3c ASCII infohash
                    + tag-02 key=1 (已下载 piece 数) + peer 缓存

断言:
  - 诊断模式: hit_rate ≥ 80%, passed=True
  - 转换模式: fastresume 生成, pieces 位图 bit 数 == 下载 piece 数
"""
import hashlib
import json
import struct
import subprocess
import sys
from pathlib import Path

import libtorrent as lt

if sys.stdout and hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

OUT = Path(__file__).resolve().parent / "e2e_out"
OUT.mkdir(exist_ok=True, parents=True)

# === 1. 源文件 + .torrent (2MB, 256KB piece → 8 pieces) ===
FILE_SIZE = 2 * 1024 * 1024
PIECE_LENGTH = 256 * 1024
NUM_PIECES = FILE_SIZE // PIECE_LENGTH  # 8
HALF = NUM_PIECES // 2                  # 前 4 个完成 (50%)

file_path = OUT / "source_file.bin"
with open(file_path, "wb") as f:
    for i in range(NUM_PIECES):
        f.write(struct.pack("<I", i) * (PIECE_LENGTH // 4))

fs = lt.file_storage()
fs.add_file("source_file.bin", FILE_SIZE)
fs.set_piece_length(PIECE_LENGTH)
t = lt.create_torrent(fs)
lt.set_piece_hashes(t, str(file_path.parent))
torrent_path = OUT / "test.torrent"
torrent_path.write_bytes(lt.bencode(t.generate()))

info = lt.torrent_info(str(torrent_path))
info_hash_bytes = info.info_hashes().v1.to_bytes()
info_hash_hex = info_hash_bytes.hex()

raw = torrent_path.read_bytes()
def bdecode(data, pos=0):
    c = chr(data[pos])
    if c == "d":
        pos += 1; d = {}
        while data[pos] != ord("e"):
            k, pos = bdecode(data, pos); v, pos = bdecode(data, pos); d[k] = v
        return d, pos + 1
    elif c == "l":
        pos += 1; l = []
        while data[pos] != ord("e"):
            v, pos = bdecode(data, pos); l.append(v)
        return l, pos + 1
    elif c == "i":
        end = data.index(b"e", pos); return int(data[pos + 1:end]), end + 1
    else:
        colon = data.index(b":", pos); n = int(data[pos:colon]); s = colon + 1
        return data[s:s + n], s + n
info_dict = bdecode(raw)[0][b"info"]
real_plen = info_dict[b"piece length"]
pieces_hash = info_dict[b"pieces"]
real_n = len(pieces_hash) // 20
print(f"[1] torrent: {real_n} pieces, piece_length={real_plen}, infohash={info_hash_hex}")

# === 2. .bt.xltd: 4096 对齐位置镜像, 零填充空洞 (非 sparse, 与真实一致) ===
xltd_path = OUT / "test.bt.xltd"
aligned = (FILE_SIZE + 4095) // 4096 * 4096
real_n = len(pieces_hash) // 20      # libtorrent 实际 piece 数 (可能非 PIECE_LENGTH)
half_real = real_n // 2              # 前一半完成 (与 key=1 计数一致)
with open(xltd_path, "wb") as f:
    with open(file_path, "rb") as src:
        f.write(src.read(half_real * real_plen))        # 前半真实数据
        f.write(b"\x00" * (aligned - half_real * real_plen))  # 其余零填充
print(f"[2] xltd: {xltd_path} size={aligned} (4096 对齐, 无头部; {half_real}/{real_n} pieces 完成)")

# === 3. .xlbt.cfg: 真实格式 ===
cfg = bytearray()
cfg += b"XDLCTX\x00\x00"          # +0x00 magic (真实!)
cfg += b"\x00" * 16                # +0x08 随机区
cfg += struct.pack("<I", 30025)    # +0x18 观测值
cfg += struct.pack("<I", 7)        # +0x1c 观测值
cfg += struct.pack("<I", 0)        # +0x20
cfg += struct.pack("<I", 4)        # +0x24
cfg += struct.pack("<I", 0)        # +0x28
cfg += struct.pack("<I", 28584)    # +0x2c 观测值
cfg += struct.pack("<I", 4)        # +0x30
cfg += struct.pack("<I", 262145)   # +0x34
cfg += struct.pack("<I", 40)       # +0x38 infohash 长度
cfg += info_hash_hex.upper().encode()  # +0x3c ASCII infohash
assert len(cfg) == 0x64
cfg += struct.pack("<HHI", 0x0002, 1, half_real)  # key=1 → 已下载 piece 数
cfg += struct.pack("<HHI", 0x0002, 2, 0)          # key=2 → 0
cfg += b"bt://127.0.0.1:51413"      # peer 缓存 (TLV 简化)
cfg_path = OUT / "test.xlbt.cfg"
cfg_path.write_bytes(cfg)
print(f"[3] cfg: {cfg_path} ({len(cfg)} bytes, magic=XDLCTX)")

# === 4. 诊断模式 ===
converter = str(Path(__file__).resolve().parent / "xunlei_to_libtorrent_converter.py")
diag_out = OUT / "diagnostic_out"
r = subprocess.run(
    [sys.executable, converter, "--torrent", str(torrent_path), "--cfg", str(cfg_path),
     "--bt-xltd", str(xltd_path), "--output-dir", str(diag_out)],
    capture_output=True, text=True, encoding="utf-8", errors="replace")
print("=== 诊断输出 ===")
print(r.stdout[-2000:])
if r.stderr:
    print("STDERR:", r.stderr[-800:])
diag = json.loads((diag_out / "conversion_diagnostic.json").read_text(encoding="utf-8"))
assert diag["validation"]["passed"], "diagnostic passed 应为 True"
assert diag["validation"]["hit_rate"] == 1.0, "合成样本命中率应为 100%"

# === 5. 转换模式 ===
conv_out = OUT / "convert_out"
r = subprocess.run(
    [sys.executable, converter, "--torrent", str(torrent_path), "--cfg", str(cfg_path),
     "--bt-xltd", str(xltd_path), "--output-dir", str(conv_out), "--convert"],
    capture_output=True, text=True, encoding="utf-8", errors="replace")
print("=== 转换输出 ===")
print(r.stdout[-1500:])
if r.stderr:
    print("STDERR:", r.stderr[-800:])
report = json.loads((conv_out / "conversion_report.json").read_text(encoding="utf-8"))
assert report["status"] == "OK", f"转换应 OK: {report.get('reason')}"
assert report["pieces_done"] == half_real, f"位图应标记 {half_real} 个完成 piece"

# === 6. libtorrent 回读 fastresume 校验 bitfield ===
import libtorrent as lt2
fr_path = conv_out / f"{lt.torrent_info(str(torrent_path)).name()}.fastresume"
fr = lt2.bdecode(fr_path.read_bytes())
bits = fr[b"pieces"]
set_bits = sum(bin(b).count("1") for b in bits)
assert set_bits == half_real, f"fastresume bitfield 应有 {half_real} bit, got {set_bits}"
print(f"[OK] fastresume bitfield: {set_bits}/{real_n} pieces 完成, 与源一致")

print("\n" + "=" * 60)
print("E2E 通过: 真实格式合成样本 → 诊断+转换 全流程 OK")
print("=" * 60)
for p in sorted(OUT.rglob("*")):
    if p.is_file():
        print(f"  {p.relative_to(OUT)}  ({p.stat().st_size} bytes)")