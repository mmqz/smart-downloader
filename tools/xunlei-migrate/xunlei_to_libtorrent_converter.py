"""
迅雷 BT 任务 → libtorrent fastresume 转换器 (真实样本验证版)

2026-08-17 重构: 基于真实样本 C5AA149AE0776344A270EAFEE49FDADB43FF6097
验证结果 (validate_xunlei_sample.py V1-V8 全绿), 替换被证伪的合成模型。

真实格式事实 (A 级, 详见 spec_pending_validation.md):
  - .xlbt.cfg: magic="XDLCTX\\x00\\x00", 0x3c 起 40B ASCII infohash,
    tag-02 int 记录 (key=1 = 已下载 piece 数), peer 缓存 "bt://ip:port",
    **无 piece 哈希表, 无 bitfield** (任务元数据文件)
  - .bt.xltd: 文件的位置镜像 (byte x of xltd = byte x of file),
    大小 = ceil(file_size/4096)*4096, 无头部, 整文件预分配 (零填充空洞),
    piece p 数据在 xltd 偏移 = p*piece_length - file_start_offset (内部 piece)
  - 完成位图: 只能由 xltd 数据 + torrent piece 哈希 SHA1 验算推导

转换策略 (本文件):
  1. 读 .torrent → piece_length / pieces_hash / info_hash / 文件偏移表
  2. 读 .xlbt.cfg → 校验 magic + infohash 一致性
  3. 对每个 .bt.xltd: 按 4096 对齐尺寸匹配 torrent 文件 → 逐 piece SHA1
     → 推导完成位图 (仅哈希一致的 piece 置 1; 在途 piece 视为未完成)
  4. 生成 libtorrent fastresume (v1): info-hash + pieces 位图 + 文件尺寸
  5. 若目标数据文件缺失, 从 .xltd 物化 (copy)

用法:
  python xunlei_to_libtorrent_converter.py --torrent <t> --cfg <c> \
      --bt-xltd <x1> [--bt-xltd <x2> ...] [--xltd-dir <dir>] \
      [--output-dir <dir>] [--convert]
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import sys
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

MAGIC_REAL = b"XDLCTX\x00\x00"
ALIGN = 4096
INFO_HASH_OFF = 0x3C
INT_TAG = b"\x02\x00"
HIT_RATE_MIN = 0.80
MATCH_MIN = 30


# ============= 通用 bencode =============
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


def bencode(v):
    if isinstance(v, dict):
        return b"d" + b"".join(bencode(k) + bencode(v[k]) for k in sorted(v)) + b"e"
    elif isinstance(v, list):
        return b"l" + b"".join(bencode(x) for x in v) + b"e"
    elif isinstance(v, int):
        return b"i" + str(v).encode() + b"e"
    elif isinstance(v, bytes):
        return str(len(v)).encode() + b":" + v
    raise TypeError(f"can't bencode {type(v)}")


# ============= .torrent 解析 =============
def parse_torrent(path: Path) -> Dict[str, Any]:
    import libtorrent as lt
    ti = lt.torrent_info(str(path))
    raw = path.read_bytes()
    info = bdecode(raw)[0][b"info"]
    pieces_hash = info[b"pieces"]
    plen = info[b"piece length"]
    files = []
    off = 0
    if b"files" in info:
        for f in info[b"files"]:
            name = "/".join(x.decode(errors="replace") for x in f[b"path"])
            files.append({"name": name, "offset": off, "size": f[b"length"]})
            off += f[b"length"]
    else:  # 单文件种子
        name = info.get(b"name", b"single").decode(errors="replace")
        files.append({"name": name, "offset": 0, "size": info[b"length"]})
        off = info[b"length"]
    return {
        "name": ti.name(),
        "info_hash": ti.info_hashes().v1.to_bytes(),
        "info_hash_hex": ti.info_hashes().v1.to_bytes().hex(),
        "piece_length": plen,
        "num_pieces": len(pieces_hash) // 20,
        "pieces_hash": pieces_hash,
        "files": files,
        "total_size": off,
    }


# ============= .xlbt.cfg 解析 (真实格式) =============
def parse_cfg(path: Path) -> Dict[str, Any]:
    data = path.read_bytes()
    out = {"size": len(data), "magic": data[0:8], "magic_ok": data[0:8] == MAGIC_REAL}
    if len(data) >= INFO_HASH_OFF + 40:
        out["infohash_ascii"] = data[INFO_HASH_OFF:INFO_HASH_OFF + 40].decode(errors="replace")
    i = 0x64
    cnt = None
    while i + 8 <= len(data) and data[i:i + 2] == INT_TAG:
        key = struct.unpack("<H", data[i + 2:i + 4])[0]
        val = struct.unpack("<I", data[i + 4:i + 8])[0]
        if key == 1:
            cnt = val
        i += 8
    out["downloaded_piece_count"] = cnt
    out["peers"] = [m.group().decode() for m in re.finditer(rb"bt://[\d.]+:\d+", data)]
    return out


# ============= .bt.xltd 探测 + piece 验证 =============
def match_torrent_file(xltd_size: int, files: List[Dict]) -> Optional[Dict]:
    for f in files:
        if (f["size"] + ALIGN - 1) // ALIGN * ALIGN == xltd_size:
            return f
    return None


def verify_xltd(xltd: Path, torrent: Dict[str, Any]) -> Dict[str, Any]:
    """逐 piece SHA1 验证单个 .xltd → 完成位图 + 统计"""
    sz = xltd.stat().st_size
    fi = match_torrent_file(sz, torrent["files"])
    if fi is None:
        return {"file": None, "error": f"xltd size {sz} 不匹配任何 torrent 文件 (4096 对齐)"}
    data = xltd.read_bytes()
    plen = torrent["piece_length"]
    pieces = torrent["pieces_hash"]
    last = torrent["num_pieces"] - 1
    bitmap = [0] * (last + 1)
    match = partial = allzero = 0
    for p in range((fi["offset"] + plen - 1) // plen, last + 1):
        s = p * plen - fi["offset"]
        if s < 0 or s >= fi["size"]:
            continue
        chunk = data[s:s + plen]
        if hashlib.sha1(chunk).digest() == pieces[p * 20:(p + 1) * 20]:
            bitmap[p] = 1
            match += 1
        elif any(chunk):
            partial += 1
        else:
            allzero += 1
    return {
        "file": fi["name"], "file_size": fi["size"], "xltd_size": sz,
        "match": match, "partial": partial, "allzero": allzero,
        "bitmap": bitmap,
    }


def materialized_data_file(xltd: Path) -> Optional[Path]:
    """.bt.xltd 同目录同名去掉后缀 = 迅雷物化的目标数据文件 (若有)"""
    cand = xltd.with_name(xltd.name[: -len(".bt.xltd")])
    return cand if cand.exists() else None


# ============= 验证流程 (转换门禁) =============
def run_validation(torrent: Dict, cfg: Dict, xltds: List[Path]) -> Dict[str, Any]:
    match_tot = partial_tot = 0
    per_file = []
    for x in xltds:
        r = verify_xltd(x, torrent)
        if "error" in r:
            per_file.append(r)
            continue
        match_tot += r["match"]; partial_tot += r["partial"]
        per_file.append(r)
    rate = match_tot / (match_tot + partial_tot) if (match_tot + partial_tot) else 0.0
    ih_ok = (cfg.get("infohash_ascii") or "").lower() == torrent["info_hash_hex"]
    v = {
        "magic_ok": cfg["magic_ok"],
        "infohash_match": ih_ok,
        "piece_match": match_tot,
        "piece_partial": partial_tot,
        "hit_rate": rate,
        "per_file": per_file,
    }
    v["passed"] = (
        cfg["magic_ok"] and ih_ok and match_tot >= MATCH_MIN and rate >= HIT_RATE_MIN
    )
    return v


# ============= 转换 =============
def do_convert(torrent_path: Path, cfg_path: Path, xltds: List[Path],
               output_dir: Path, allow_unverified: bool = False) -> Dict[str, Any]:
    output_dir.mkdir(exist_ok=True, parents=True)
    torrent = parse_torrent(torrent_path)
    cfg = parse_cfg(cfg_path)
    validation = run_validation(torrent, cfg, xltds)
    if not validation["passed"] and not allow_unverified:
        return {"status": "REFUSED", "reason": "validation not passed",
                "validation": validation}

    # 完成位图 = 各 xltd SHA1 验算合并 (多文件任务, piece 空间全局)
    bitmap = [0] * torrent["num_pieces"]
    for r in validation["per_file"]:
        if "error" not in r:
            for p, b in enumerate(r["bitmap"]):
                bitmap[p] = max(bitmap[p], b)
    bitfield = bytearray((torrent["num_pieces"] + 7) // 8)
    for p, b in enumerate(bitmap):
        if b:
            bitfield[p // 8] |= 1 << (7 - (p % 8))

    # fastresume v1
    fr = {
        b"file-format": b"libtorrent resume file",
        b"file-version": 1,
        b"info-hash": torrent["info_hash"],
        b"pieces": bytes(bitfield),
        b"name": torrent["name"].encode("utf-8"),
        b"save_path": str(output_dir).encode("utf-8"),
        b"total_uploaded": 0,
        b"upload-mode": 0,
        b"file sizes": [[f["size"], 0] for f in torrent["files"]],
    }
    fr_path = output_dir / f"{torrent['name']}.fastresume"
    fr_path.write_bytes(bencode(fr))

    # 物化缺失的数据文件 (xltd → 目标文件, 去 4096 填充)
    materialized = []
    for x, r in zip(xltds, validation["per_file"]):
        if "error" in r:
            continue
        target = materialized_data_file(x)
        if target is None:
            dst = output_dir / Path(r["file"]).name
            data = x.read_bytes()
            dst.write_bytes(data[:r["file_size"]])
            materialized.append(str(dst))
        else:
            materialized.append(f"(已存在) {target}")

    return {
        "status": "OK",
        "fastresume": str(fr_path),
        "bitfield_size": len(bitfield),
        "pieces_total": torrent["num_pieces"],
        "pieces_done": sum(bitmap),
        "hit_rate": validation["hit_rate"],
        "info_hash": torrent["info_hash_hex"],
        "materialized": materialized,
        "validation": validation,
        "note": "qBittorrent: 添加 .torrent → 选数据目录 → 自动 rehash 校验, 缺失 piece 会补下",
    }


def main():
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass
    ap = argparse.ArgumentParser(description="迅雷 → libtorrent 转换器 (真实格式版)")
    ap.add_argument("--torrent", required=True)
    ap.add_argument("--cfg", required=True)
    ap.add_argument("--bt-xltd", action="append", default=[])
    ap.add_argument("--xltd-dir", default=None)
    ap.add_argument("--output-dir", default="./output")
    ap.add_argument("--convert", action="store_true")
    ap.add_argument("--allow-unverified", action="store_true")
    args = ap.parse_args()

    xltds = list(args.bt_xltd)
    if args.xltd_dir:
        for dp, _, fn in os.walk(args.xltd_dir):
            for f in fn:
                if f.endswith(".bt.xltd"):
                    xltds.append(str(Path(dp) / f))
    if not xltds:
        print("[ERR] 至少需要一个 --bt-xltd 或 --xltd-dir")
        sys.exit(1)

    torrent = parse_torrent(Path(args.torrent))
    cfg = parse_cfg(Path(args.cfg))
    out = Path(args.output_dir)

    if not args.convert:
        v = run_validation(torrent, cfg, [Path(x) for x in xltds])
        print("=== 诊断 (真实格式) ===")
        print(f"  torrent: {torrent['name']} infohash={torrent['info_hash_hex']}")
        print(f"  files:   {torrent['files']}")
        print(f"  cfg:     magic_ok={cfg['magic_ok']} infohash={cfg.get('infohash_ascii')} "
              f"downloaded_piece_count={cfg.get('downloaded_piece_count')} peers={len(cfg.get('peers', []))}")
        for r in v["per_file"]:
            if "error" in r:
                print(f"  xltd: {r}")
            else:
                print(f"  xltd {r['file']}: match={r['match']} partial={r['partial']} "
                      f"allzero={r['allzero']} (n={r['match'] + r['partial'] + r['allzero']})")
        print(f"  hit_rate: {v['hit_rate']:.1%}  (match {v['piece_match']} / 完成+在途 {v['piece_match'] + v['piece_partial']})")
        print(f"  passed: {v['passed']}")
        out.mkdir(exist_ok=True, parents=True)
        (out / "conversion_diagnostic.json").write_text(
            json.dumps({"torrent": {k: torrent[k] for k in ("name", "info_hash_hex", "piece_length", "num_pieces")},
                        "validation": v, "cfg": {k: cfg[k] for k in ("size", "magic_ok", "downloaded_piece_count")}},
                       indent=2, ensure_ascii=False, default=str), encoding="utf-8")
        print(f"[OK] 诊断报告: {out / 'conversion_diagnostic.json'}")
        if v["passed"]:
            print("✅ 验证通过! 可加 --convert 生成 fastresume")
        else:
            print("⚠ 验证未通过 (见上)")
        return

    print("=== 转换模式 ===")
    result = do_convert(Path(args.torrent), Path(args.cfg), [Path(x) for x in xltds], out,
                        allow_unverified=args.allow_unverified)
    print(json.dumps({k: v for k, v in result.items() if k != "validation"}, indent=2, ensure_ascii=False, default=str))
    (out / "conversion_report.json").write_text(
        json.dumps(result, indent=2, ensure_ascii=False, default=str), encoding="utf-8")
    print(f"[OK] 转换报告: {out / 'conversion_report.json'}")


if __name__ == "__main__":
    main()