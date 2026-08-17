"""
底层逆向分析器: 从真实 .xlbt.cfg 样本提取已确认的字段 (A 级证据)

只做"可证明"的提取: 与 .torrent/.bt.xltd 交叉验证过的字段才有资格进入
parse_xlbt_cfg.py 的正式解析。无法解释的字节一律报告为 unknown/offset。

用法:
  python analyze_xlbt_cfg.py <cfg> <torrent> [<xltd-dir>]
"""
import argparse
import hashlib
import json
import os
import re
import struct
import sys
from pathlib import Path


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


def torrent_meta(path: Path):
    raw = path.read_bytes()
    info = bdecode(raw)[0][b"info"]
    pieces = info[b"pieces"]
    plen = info[b"piece length"]
    files = []
    off = 0
    for f in info[b"files"]:
        p = f[b"path"]
        name = "/".join(x.decode(errors="replace") for x in p)
        files.append({"name": name, "offset": off, "size": f[b"length"]})
        off += f[b"length"]
    v1 = None  # infohash 由 validate_xunlei_sample.py 用 libtorrent 计算
    return {
        "piece_length": plen,
        "num_pieces": len(pieces) // 20,
        "pieces": pieces,
        "files": files,
        "total": off,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cfg")
    ap.add_argument("torrent")
    ap.add_argument("--xltd-dir", default=None, help="含 .bt.xltd 的目录(可选,验证文件大小对齐)")
    args = ap.parse_args()

    data = Path(args.cfg).read_bytes()
    tm = torrent_meta(Path(args.torrent))
    out = {"cfg_size": len(data), "torrent": {
        "piece_length": tm["piece_length"], "num_pieces": tm["num_pieces"],
        "files": tm["files"], "total": tm["total"]}, "found": {}}

    # 1) magic
    out["found"]["magic"] = {"offset": 0x00, "value": data[0:8].hex()}
    # 2) 头部 16B 随机区(无法解释,报告值)
    out["found"]["head_8_18"] = {"offset": 0x08, "value": data[0x08:0x18].hex()}
    # 3) 头部前导字段值(u32 候选)
    for off in range(0x18, 0x3c, 4):
        v = struct.unpack("<I", data[off:off + 4])[0]
        out["found"][f"u32@{off:#06x}"] = v
    # 4) infohash ASCII
    ih = data[0x3c:0x3c + 40].decode(errors="replace")
    out["found"]["infohash_ascii"] = {"offset": 0x3c, "value": ih}
    # 5) int 记录: [02 00 <key16> <val32>] 模式扫描, 收集非零值
    ints = []
    i = 0x64
    while i + 8 <= len(data):
        if data[i] == 0x02 and data[i + 1] == 0x00:
            key = struct.unpack("<H", data[i + 2:i + 4])[0]
            val = struct.unpack("<I", data[i + 4:i + 8])[0]
            ints.append((i, key, val))
            i += 8
        else:
            i += 1
    nz = [(o, k, v) for o, k, v in ints if v != 0]
    print(f"int records (tag02): total={len(ints)} nonzero={nz[:20]}")
    out["found"]["int_nz"] = [[o, k, v] for o, k, v in nz[:20]]
    # 6) 文件大小 u64 候选 (从 0x6f00 开始向后找 4 个已知大小)
    sizes = [f["size"] for f in tm["files"]]
    for want in sizes:
        pat = struct.pack("<Q", want)
        idx = data.find(pat)
        out["found"].setdefault("file_sizes", []).append(
            {"size": want, "offset": idx})
    # 7) peer 缓存 "bt://"
    peers = [(m.start(), m.group().decode()) for m in re.finditer(rb"bt://[\d.]+:\d+", data)]
    out["found"]["peers"] = [[o, s] for o, s in peers]
    # 8) Reserved 标签
    res = [(m.start(), m.end()) for m in re.finditer(rb"Reserved", data)]
    out["found"]["reserved_blobs"] = res
    # 9) 已完成 piece 数交叉验证: 从 xltd 目录内搜索与 torrent 匹配的文件推导
    if args.xltd_dir:
        hit = 0
        total = 0
        for dp, _, fn in os.walk(args.xltd_dir):
            for f in fn:
                if f.endswith(".bt.xltd"):
                    xl = Path(dp) / f
                    sz = xl.stat().st_size
                    # 找匹配的 torrent 文件 (4096 对齐)
                    for fi in tm["files"]:
                        if (fi["size"] + 4095) // 4096 * 4096 == sz:
                            data2 = xl.read_bytes()
                            p0 = (fi["offset"] + tm["piece_length"] - 1) // tm["piece_length"]
                            p1 = len(tm["pieces"]) // 20 - 1
                            for p in range(p0, p1 + 1):
                                s = p * tm["piece_length"] - fi["offset"]
                                if s < 0 or s >= fi["size"]:
                                    continue
                                chunk = data2[s:s + tm["piece_length"]]
                                if hashlib.sha1(chunk).digest() == tm["pieces"][p * 20:(p + 1) * 20]:
                                    hit += 1
                                total += 1
        out["found"]["piece_verify"] = {"hit": hit, "checked": total}
    Path("samples/analysis.json").write_text(json.dumps(out, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps(out["found"], indent=2, ensure_ascii=False)[:4000])


if __name__ == "__main__":
    main()