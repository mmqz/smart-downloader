"""
PoC: 迅雷 BT 任务 → libtorrent fastresume 转换器 (重写版)

⚠ 警告 ⚠
本程序是 PoC,基于反汇编推断实现。所有 section_id 映射 / .bt.xltd 偏移布局
均为 C/D 级推断 (见 spec_pending_validation.md),必须先运行
validate_xunlei_sample.py 验证通过后才可启用真实转换。

默认运行模式 = DIAGNOSTIC (诊断),只输出 JSON 报告,不产生任何文件变更。
显式传入 --convert 才会执行真实转换,且每个假设分支都有验证检查。

输入:
  --torrent <path>      原始 .torrent 文件
  --bt-xltd <path>      迅雷 .bt.xltd 文件
  --cfg <path>          迅雷 .xlbt.cfg 文件
  --output-dir <path>   输出目录 (默认 ./output)
  --convert             启用真实转换 (默认禁用,只诊断)
  --allow-unverified     即使部分假设未验证也强制转换 (危险,仅用于测试)

输出 (DIAGNOSTIC 模式):
  - conversion_diagnostic.json: 完整诊断报告

输出 (CONVERT 模式,所有验证通过后):
  - <task_name>.fastresume: libtorrent fastresume bencode
  - <task_name>.part: 重命名自 .bt.xltd (libtorrent 标准 .part 文件)
  - conversion_report.json: 转换结果

证据等级参考:
  [A] 直接反汇编验证 (magic, 头部, section entry 结构)
  [B] 多个独立证据支持 (.bt.xltd 纯数据推断)
  [C] 单一间接证据 / 推断 (section_id 映射)
  [D] 纯命名推测 (CXBitmap 字节序)
"""
import argparse
import json
import os
import struct
import sys
import hashlib
from pathlib import Path
from dataclasses import dataclass, asdict, field
from typing import Optional, List, Dict, Any, Tuple

# ============= A 级常量 (反汇编已确认) =============
EXPECTED_MAGIC = b"XLBTCFG\x00"
HEADER_SIZE = 0x28        # 40 bytes
SECTION_ENTRY_SIZE = 0x14  # 20 bytes
BLOCK_SIZE_ALIGNMENT = 0x1000  # 4096

# ============= C/D 级推测 (见 spec_pending_validation.md) =============
# ⚠ 警告: 以下 section_id 数值是纯猜测,无反汇编证据
# 必须先运行 validate_xunlei_sample.py 验证后才能用
SECTION_ID_INFO_HASH    = 0x00000001   # D 级猜测
SECTION_ID_PIECES_HASH  = 0x00000002   # D 级猜测
SECTION_ID_BITFIELD     = 0x00000003   # D 级猜测
SECTION_ID_FILE_INFO    = 0x00000004   # D 级猜测
SECTION_ID_GCID         = 0x00000005   # D 级猜测


# ============= 数据结构 =============
@dataclass
class CfgHeader:
    magic: bytes
    reserved1: int
    reserved2: int
    reserved3: int
    block_count: int
    block_size: int
    section_count: int
    reserved4: int
    magic_ok: bool
    block_size_aligned: bool


@dataclass
class CfgSection:
    index: int
    offset_in_file: int
    section_id: int
    field2: int   # 推测: size (C 级)
    field3: int   # 推测: offset (C 级)


@dataclass
class CfgParseResult:
    header: CfgHeader
    sections: List[CfgSection]
    raw_size: int
    raw_data: bytes  # 用于 section 内容读取


@dataclass
class ValidationReport:
    """验证报告 - 标记每个推测的验证结果"""
    # 头部
    magic_ok: bool = False
    block_size_aligned: bool = False
    section_count_reasonable: bool = False
    # section 映射 (C/D 级)
    info_hash_section_found: bool = False
    pieces_hash_section_found: bool = False
    bitfield_section_found: bool = False
    # xltd 验证
    xltd_no_magic: bool = False       # B 级推断
    xltd_sparse: bool = False
    piece_offset_verified: bool = False  # 关键: piece hash 比对
    piece_hash_match_rate: float = 0.0   # 命中率
    # 字段交叉验证
    info_hash_matches_torrent: bool = False
    pieces_hash_matches_torrent: bool = False
    # 总结
    all_critical_verified: bool = False
    can_proceed_convert: bool = False


# ============= 解析函数 =============
def parse_xlbt_cfg(path: Path) -> CfgParseResult:
    """解析 .xlbt.cfg 头部 + section 数组 (A 级字段)"""
    data = path.read_bytes()
    size = len(data)
    if size < HEADER_SIZE:
        raise ValueError(f"file too small: {size} < {HEADER_SIZE}")
    
    magic = data[0:8]
    reserved1 = struct.unpack("<H", data[8:10])[0]
    reserved2 = struct.unpack("<H", data[10:12])[0]
    reserved3 = struct.unpack("<I", data[12:16])[0]
    block_count = struct.unpack("<Q", data[16:24])[0]
    block_size = struct.unpack("<Q", data[24:32])[0]
    section_count = struct.unpack("<I", data[32:36])[0]
    reserved4 = struct.unpack("<I", data[36:40])[0]
    
    header = CfgHeader(
        magic=magic,
        reserved1=reserved1, reserved2=reserved2, reserved3=reserved3,
        block_count=block_count,
        block_size=block_size,
        section_count=section_count,
        reserved4=reserved4,
        magic_ok=(magic == EXPECTED_MAGIC),
        block_size_aligned=(block_size % BLOCK_SIZE_ALIGNMENT == 0),
    )
    
    sections = []
    for i in range(min(section_count, 1000)):
        offset = HEADER_SIZE + i * SECTION_ENTRY_SIZE
        if offset + SECTION_ENTRY_SIZE > size:
            break
        section_id = struct.unpack("<I", data[offset:offset+4])[0]
        field2 = struct.unpack("<Q", data[offset+4:offset+12])[0]
        field3 = struct.unpack("<Q", data[offset+12:offset+20])[0]
        sections.append(CfgSection(
            index=i,
            offset_in_file=offset,
            section_id=section_id,
            field2=field2,
            field3=field3,
        ))
    
    return CfgParseResult(header=header, sections=sections, raw_size=size, raw_data=data)


def find_section_by_id(cfg: CfgParseResult, section_id: int) -> Optional[CfgSection]:
    """按 section_id 查找 (C 级: section_id 数值是猜测)"""
    for s in cfg.sections:
        if s.section_id == section_id:
            return s
    return None


def read_section_body(cfg: CfgParseResult, section: CfgSection) -> bytes:
    """读 section 内容
    
    ⚠ C 级推断: 假设 field2 = size, field3 = offset
    必须先验证此假设!
    """
    size = section.field2
    offset = section.field3
    if offset + size > cfg.raw_size:
        return b""
    return cfg.raw_data[offset:offset+size]


# ============= .torrent 解析 (用 libtorrent) =============
def parse_torrent(path: Path) -> Optional[Dict[str, Any]]:
    """用 libtorrent 解析 .torrent 文件"""
    try:
        import libtorrent as lt
    except ImportError:
        print("[WARN] libtorrent not installed, torrent parse skipped")
        return None
    
    info = lt.torrent_info(str(path))
    files = info.files()
    
    # 拿 piece hashes (标准 SHA1 列表)
    # libtorrent Python binding 不直接暴露 piece hashes,需用 bencode 解析
    torrent_data = path.read_bytes()
    
    # 简单 bencode 解析 (只取 info dict 的 pieces 字段)
    def bdecode(data: bytes, pos: int = 0) -> Tuple[Any, int]:
        c = chr(data[pos])
        if c == 'd':
            pos += 1
            d = {}
            while data[pos] != ord('e'):
                k, pos = bdecode(data, pos)
                v, pos = bdecode(data, pos)
                d[k] = v
            return d, pos + 1
        elif c == 'l':
            pos += 1
            l = []
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
        else:
            raise ValueError(f"bad bencode at {pos}: {c}")
    
    parsed, _ = bdecode(torrent_data)
    info_dict = parsed[b'info']
    pieces_hash = info_dict[b'pieces']  # bytes, length = num_pieces * 20
    piece_length = info_dict[b'piece length']
    
    # 计算 infohash
    # 直接用 libtorrent 算的 info_hash (避免 bencode 重算不一致)
    info_hash = info.info_hash().to_bytes()
    
    return {
        "name": info.name(),
        "info_hash": info_hash,
        "info_hash_hex": info_hash.hex(),
        "piece_length": piece_length,
        "num_pieces": info.num_pieces(),
        "total_size": info.total_size(),
        "num_files": info.num_files(),
        "pieces_hash": pieces_hash,  # raw bytes
        "pieces_hash_hex_head": pieces_hash[:60].hex(),
        "files": [
            {"path": files.at(i).path, "size": files.at(i).size}
            for i in range(info.num_files())
        ],
    }


# ============= .bt.xltd 探测 =============
def probe_bt_xltd(path: Path) -> Dict[str, Any]:
    """探测 .bt.xltd 文件结构 (B 级推断: 纯数据 sparse file)"""
    p = path
    size = p.stat().st_size
    
    with open(path, 'rb') as f:
        head = f.read(64)
    
    # 检查前 8 字节是否 ASCII magic
    is_ascii_magic = (
        len(head) >= 8 and
        all(32 <= b < 127 for b in head[0:8]) and
        head[0:8] != b'\x00' * 8
    )
    
    # 检查 sparse
    stat = p.stat()
    actual_blocks = getattr(stat, 'st_blocks', None)
    actual_bytes = actual_blocks * 512 if actual_blocks is not None else size
    is_sparse = actual_blocks is not None and actual_blocks * 512 < size
    
    return {
        "path": str(path),
        "size": size,
        "actual_disk_bytes": actual_bytes,
        "is_sparse": is_sparse,
        "is_ascii_magic": is_ascii_magic,
        "first8_hex": head[0:8].hex(),
        "first64_hex": head.hex(),
    }


# ============= 关键验证: piece hash 比对 =============
def verify_piece_offset(
    bt_xltd_path: Path,
    pieces_hash: bytes,
    piece_length: int,
    num_pieces: int,
    sample_count: int = 10,
    bitfield: Optional[bytes] = None,
) -> Tuple[bool, float, List[Dict]]:
    """验证 .bt.xltd 偏移布局是否 = piece_index × piece_length
    
    策略: 从 .bt.xltd 抽取若干 piece 数据,算 SHA1,与 pieces_hash 比对
    
    关键:
      - 已下载 piece (bitfield 标记 1): 必须匹配
      - 未下载 piece (bitfield 标记 0): 跳过 (sparse hole 区域不应算未命中)
    
    若无 bitfield 输入,则只检查前 N 个 piece (推测已下载)
    
    命中率 = 匹配的 piece 数 / 检查的 piece 数 (只算已下载的)
    通过条件: 命中率 ≥ 80% 且 至少 5 个已下载 piece 被检查
    
    返回: (是否通过, 命中率, 详细结果)
    """
    size = bt_xltd_path.stat().st_size
    expected_size = num_pieces * piece_length
    if size < expected_size:
        max_checkable = size // piece_length
    else:
        max_checkable = num_pieces
    
    if max_checkable == 0:
        return False, 0.0, []
    
    # 决定哪些 piece 需要检查
    def is_piece_complete(idx: int) -> bool:
        """bitfield 检查 piece 是否已下载"""
        if bitfield is None:
            # 无 bitfield, 假设前 N 个 piece 都已下载 (验证用)
            return idx < max_checkable // 2
        byte_idx = idx // 8
        bit_idx = 7 - (idx % 8)  # big-endian (标准 BT)
        if byte_idx >= len(bitfield):
            return False
        return bool(bitfield[byte_idx] & (1 << bit_idx))
    
    # 找已下载的 piece 索引
    completed_indices = [i for i in range(num_pieces) if is_piece_complete(i)]
    if len(completed_indices) < 5:
        # 已下载 piece 太少, 无法验证
        return False, 0.0, []
    
    # 从已下载 piece 里采样
    if sample_count > len(completed_indices):
        sample_count = len(completed_indices)
    step = max(1, len(completed_indices) // sample_count)
    sample_indices = completed_indices[::step][:sample_count]
    
    results = []
    matches = 0
    with open(bt_xltd_path, 'rb') as f:
        for idx in sample_indices:
            offset = idx * piece_length
            f.seek(offset)
            data = f.read(piece_length)
            if len(data) < piece_length:
                results.append({
                    "piece_index": idx,
                    "offset": offset,
                    "read_bytes": len(data),
                    "expected_hash": pieces_hash[idx*20:(idx+1)*20].hex(),
                    "actual_hash": None,
                    "match": False,
                    "reason": "short_read",
                })
                continue
            actual_hash = hashlib.sha1(data).digest()
            expected = pieces_hash[idx*20:(idx+1)*20]
            match = (actual_hash == expected)
            if match:
                matches += 1
            results.append({
                "piece_index": idx,
                "offset": offset,
                "read_bytes": len(data),
                "expected_hash": expected.hex(),
                "actual_hash": actual_hash.hex(),
                "match": match,
            })
    
    match_rate = matches / len(sample_indices) if sample_indices else 0.0
    return match_rate >= 0.8, match_rate, results


# ============= 验证流程 =============
def run_validation(
    torrent_path: Path,
    bt_xltd_path: Path,
    cfg_path: Path,
) -> Tuple[ValidationReport, Dict[str, Any]]:
    """完整验证流程"""
    report = ValidationReport()
    details = {}
    
    # === Step 1: 解析 .torrent ===
    torrent_info = parse_torrent(torrent_path)
    if not torrent_info:
        return report, {"error": "torrent parse failed (libtorrent not installed?)"}
    details["torrent"] = {
        "name": torrent_info["name"],
        "info_hash": torrent_info["info_hash_hex"],
        "piece_length": torrent_info["piece_length"],
        "num_pieces": torrent_info["num_pieces"],
        "total_size": torrent_info["total_size"],
        "pieces_hash_head": torrent_info["pieces_hash_hex_head"],
    }
    
    # === Step 2: 解析 .xlbt.cfg ===
    try:
        cfg = parse_xlbt_cfg(cfg_path)
    except Exception as e:
        return report, {"error": f"cfg parse failed: {e}"}
    
    report.magic_ok = cfg.header.magic_ok
    report.block_size_aligned = cfg.header.block_size_aligned
    report.section_count_reasonable = 0 < cfg.header.section_count < 1000
    
    details["cfg"] = {
        "size": cfg.raw_size,
        "magic_ok": cfg.header.magic_ok,
        "block_count": cfg.header.block_count,
        "block_size": cfg.header.block_size,
        "block_size_aligned": cfg.header.block_size_aligned,
        "section_count": cfg.header.section_count,
        "sections": [
            {
                "index": s.index,
                "section_id": f"0x{s.section_id:08x}",
                "field2": s.field2,
                "field3": s.field3,
            }
            for s in cfg.sections
        ],
    }
    
    # === Step 3: 探测 .bt.xltd ===
    xltd_info = probe_bt_xltd(bt_xltd_path)
    report.xltd_no_magic = not xltd_info["is_ascii_magic"]
    report.xltd_sparse = xltd_info["is_sparse"]
    details["bt_xltd"] = xltd_info
    
    # === Step 4: section_id 映射 (C/D 级, 只列推测) ===
    # 严格说这只是"按推测的 section_id 查找",不代表真实映射
    speculative_sections = {
        "INFO_HASH": find_section_by_id(cfg, SECTION_ID_INFO_HASH),
        "PIECES_HASH": find_section_by_id(cfg, SECTION_ID_PIECES_HASH),
        "BITFIELD": find_section_by_id(cfg, SECTION_ID_BITFIELD),
        "FILE_INFO": find_section_by_id(cfg, SECTION_ID_FILE_INFO),
        "GCID": find_section_by_id(cfg, SECTION_ID_GCID),
    }
    details["speculative_section_map"] = {
        k: ({"section_id": f"0x{v.section_id:08x}", "field2": v.field2, "field3": v.field3} if v else None)
        for k, v in speculative_sections.items()
    }
    
    report.info_hash_section_found = speculative_sections["INFO_HASH"] is not None
    report.pieces_hash_section_found = speculative_sections["PIECES_HASH"] is not None
    report.bitfield_section_found = speculative_sections["BITFIELD"] is not None
    
    # === Step 5: 关键验证 - piece hash 比对 ===
    # 这一步是验证 .bt.xltd 偏移布局的核心
    # 优先用 bitfield (如果 cfg 里有且能找到)
    bitfield_for_verify = None
    if report.bitfield_section_found:
        bf_section = speculative_sections["BITFIELD"]
        bf_data = read_section_body(cfg, bf_section)
        expected_bf_size = (torrent_info["num_pieces"] + 7) // 8
        if len(bf_data) == expected_bf_size:
            bitfield_for_verify = bf_data
    
    xltd_size = bt_xltd_path.stat().st_size
    if xltd_size >= torrent_info["piece_length"]:
        # 抽样 10 个 piece, 只检查 bitfield 标记为已下载的
        passed, rate, results = verify_piece_offset(
            bt_xltd_path,
            torrent_info["pieces_hash"],
            torrent_info["piece_length"],
            torrent_info["num_pieces"],
            sample_count=10,
            bitfield=bitfield_for_verify,
        )
        report.piece_offset_verified = passed
        report.piece_hash_match_rate = rate
        details["piece_offset_verification"] = {
            "passed": passed,
            "match_rate": rate,
            "used_bitfield": bitfield_for_verify is not None,
            "samples": results,
        }
    else:
        details["piece_offset_verification"] = {
            "skipped": True,
            "reason": f"xltd size {xltd_size} < piece_length {torrent_info['piece_length']}",
        }
    
    # === Step 6: 字段交叉验证 (假设 section_id 正确) ===
    if report.info_hash_section_found:
        info_hash_section = speculative_sections["INFO_HASH"]
        info_hash_body = read_section_body(cfg, info_hash_section)
        if len(info_hash_body) == 20:
            report.info_hash_matches_torrent = (info_hash_body == torrent_info["info_hash"])
            details["info_hash_match"] = {
                "from_cfg": info_hash_body.hex(),
                "from_torrent": torrent_info["info_hash_hex"],
                "match": report.info_hash_matches_torrent,
            }
    
    if report.pieces_hash_section_found:
        pieces_section = speculative_sections["PIECES_HASH"]
        pieces_body = read_section_body(cfg, pieces_section)
        expected_size = torrent_info["num_pieces"] * 20
        if len(pieces_body) == expected_size:
            report.pieces_hash_matches_torrent = (pieces_body == torrent_info["pieces_hash"])
            details["pieces_hash_match"] = {
                "from_cfg_size": len(pieces_body),
                "expected_size": expected_size,
                "match": report.pieces_hash_matches_torrent,
            }
    
    # === Step 7: 综合判定 ===
    # 关键条件: piece hash 比对通过 (这个不依赖 section_id 映射的猜测)
    report.all_critical_verified = (
        report.magic_ok and
        report.block_size_aligned and
        report.piece_offset_verified
    )
    
    # 转换可执行条件: 关键验证通过 + 至少能找到一些 section
    report.can_proceed_convert = report.all_critical_verified and (
        report.info_hash_matches_torrent or report.pieces_hash_matches_torrent
    )
    
    return report, details


# ============= 真实转换 (假设分支,默认禁用) =============
def do_convert(
    torrent_path: Path,
    bt_xltd_path: Path,
    cfg_path: Path,
    output_dir: Path,
    allow_unverified: bool = False,
) -> Dict[str, Any]:
    """真实转换流程
    
    ⚠ 假设分支: 依赖 C/D 级推断
    必须先 run_validation 通过才能调用
    """
    output_dir.mkdir(exist_ok=True, parents=True)
    
    # 先验证
    report, details = run_validation(torrent_path, bt_xltd_path, cfg_path)
    
    if not report.all_critical_verified and not allow_unverified:
        return {
            "status": "REFUSED",
            "reason": "validation not passed, cannot convert safely",
            "report": asdict(report),
            "details": details,
        }
    
    # === Step 1: 解析 .torrent 拿元信息 ===
    torrent_info = parse_torrent(torrent_path)
    if not torrent_info:
        return {"status": "ERROR", "reason": "torrent parse failed"}
    
    piece_length = torrent_info["piece_length"]
    num_pieces = torrent_info["num_pieces"]
    info_hash = torrent_info["info_hash"]
    pieces_hash = torrent_info["pieces_hash"]
    
    # === Step 2: 解析 cfg 拿 bitfield (C 级: section_id 映射是猜测) ===
    cfg = parse_xlbt_cfg(cfg_path)
    bitfield_section = find_section_by_id(cfg, SECTION_ID_BITFIELD)
    if not bitfield_section:
        return {
            "status": "REFUSED",
            "reason": f"BITFIELD section (id=0x{SECTION_ID_BITFIELD:08x}) not found",
            "note": "section_id 映射是 C 级猜测,可能全错",
        }
    
    bitfield_data = read_section_body(cfg, bitfield_section)
    expected_bitfield_size = (num_pieces + 7) // 8
    if len(bitfield_data) != expected_bitfield_size:
        return {
            "status": "REFUSED",
            "reason": f"bitfield size mismatch: got {len(bitfield_data)}, expected {expected_bitfield_size}",
            "note": "可能 CXBitmap 不是标准 bitfield, 或 section_id 映射错误",
        }
    
    # === Step 3: 重命名 .bt.xltd → .part ===
    task_name = torrent_info["name"]
    part_path = output_dir / f"{task_name}.part"
    
    # ⚠ 关键假设: .bt.xltd 的 piece 数据按 piece_index × piece_length 偏移存储
    # 如果验证通过 (piece_offset_verified=True),这个假设成立
    if not report.piece_offset_verified and not allow_unverified:
        return {
            "status": "REFUSED",
            "reason": "piece offset layout not verified",
            "note": "必须先验证 .bt.xltd 偏移布局",
        }
    
    # 复制文件 (不破坏原迅雷文件)
    import shutil
    shutil.copy2(bt_xltd_path, part_path)
    
    # === Step 4: 生成 libtorrent fastresume bencode ===
    # libtorrent fastresume 格式 (v2):
    # {
    #   'file-format': 'libtorrent resume file',
    #   'file-version': 1,
    #   'info-hash': <20 bytes>,
    #   'pieces': <bitfield bytes>,
    #   'name': <name>,
    #   'save_path': <save_path>,
    #   'total_uploaded': 0,
    #   ...
    # }
    fastresume_data = {
        b'file-format': b'libtorrent resume file',
        b'file-version': 1,
        b'info-hash': info_hash,
        b'pieces': bitfield_data,  # 标准 BT bitfield
        b'name': task_name.encode('utf-8'),
        b'save_path': str(output_dir).encode('utf-8'),
        b'total_uploaded': 0,
        b'upload-mode': 0,
        b'file sizes': [
            [torrent_info["total_size"], 0]  # [size, mtime placeholder]
        ],
    }
    
    # bencode
    def bencode(v):
        if isinstance(v, dict):
            return b'd' + b''.join(bencode(k) + bencode(v[k]) for k in sorted(v)) + b'e'
        elif isinstance(v, list):
            return b'l' + b''.join(bencode(x) for x in v) + b'e'
        elif isinstance(v, int):
            return b'i' + str(v).encode() + b'e'
        elif isinstance(v, bytes):
            return str(len(v)).encode() + b':' + v
        else:
            raise TypeError(f"can't bencode {type(v)}")
    
    fastresume_bytes = bencode(fastresume_data)
    fastresume_path = output_dir / f"{task_name}.fastresume"
    fastresume_path.write_bytes(fastresume_bytes)
    
    return {
        "status": "OK",
        "fastresume_path": str(fastresume_path),
        "part_path": str(part_path),
        "bitfield_size": len(bitfield_data),
        "pieces_count": num_pieces,
        "info_hash": info_hash.hex(),
        "note": "转换完成。请在 qBittorrent 中: 添加 .torrent 文件 → 选 .part 所在目录 → qBittorrent 会自动 rehash 已下载 piece",
        "report": asdict(report),
    }


# ============= CLI =============
def main():
    parser = argparse.ArgumentParser(description="迅雷 → libtorrent 转换器 (PoC)")
    parser.add_argument("--torrent", required=True, help=".torrent 文件路径")
    parser.add_argument("--bt-xltd", required=True, help=".bt.xltd 文件路径")
    parser.add_argument("--cfg", required=True, help=".xlbt.cfg 文件路径")
    parser.add_argument("--output-dir", default="./output", help="输出目录")
    parser.add_argument("--convert", action="store_true",
                        help="启用真实转换 (默认只诊断)")
    parser.add_argument("--allow-unverified", action="store_true",
                        help="⚠ 危险: 即使未验证也强制转换")
    args = parser.parse_args()
    
    torrent_path = Path(args.torrent)
    bt_xltd_path = Path(args.bt_xltd)
    cfg_path = Path(args.cfg)
    output_dir = Path(args.output_dir)
    
    print(f"=== 输入 ===")
    print(f"  torrent:  {torrent_path}")
    print(f"  bt.xltd:  {bt_xltd_path}")
    print(f"  cfg:      {cfg_path}")
    print(f"  output:   {output_dir}")
    print(f"  mode:     {'CONVERT' if args.convert else 'DIAGNOSTIC'}")
    print()
    
    if not args.convert:
        # === 诊断模式 ===
        print("=== 诊断模式 ===")
        report, details = run_validation(torrent_path, bt_xltd_path, cfg_path)
        
        print("\n=== 验证报告 ===")
        print(f"  magic_ok:                    {report.magic_ok}")
        print(f"  block_size_aligned:          {report.block_size_aligned}")
        print(f"  section_count_reasonable:    {report.section_count_reasonable}")
        print(f"  info_hash_section_found:     {report.info_hash_section_found}")
        print(f"  pieces_hash_section_found:   {report.pieces_hash_section_found}")
        print(f"  bitfield_section_found:      {report.bitfield_section_found}")
        print(f"  xltd_no_magic:               {report.xltd_no_magic}")
        print(f"  xltd_sparse:                 {report.xltd_sparse}")
        print(f"  piece_offset_verified:       {report.piece_offset_verified}")
        print(f"  piece_hash_match_rate:       {report.piece_hash_match_rate*100:.1f}%")
        print(f"  info_hash_matches_torrent:   {report.info_hash_matches_torrent}")
        print(f"  pieces_hash_matches_torrent: {report.pieces_hash_matches_torrent}")
        print(f"  all_critical_verified:       {report.all_critical_verified}")
        print(f"  can_proceed_convert:         {report.can_proceed_convert}")
        
        # 保存诊断报告
        output_dir.mkdir(exist_ok=True, parents=True)
        report_path = output_dir / "conversion_diagnostic.json"
        report_path.write_text(json.dumps({
            "report": asdict(report),
            "details": details,
        }, indent=2, ensure_ascii=False, default=str))
        print(f"\n[OK] 诊断报告: {report_path}")
        
        if report.can_proceed_convert:
            print("\n✅ 验证通过! 可加 --convert 启用真实转换")
        else:
            print("\n⚠ 验证未通过,推断错误或样本不完整,请检查 details")
        return
    
    # === 转换模式 ===
    print("=== 转换模式 ===")
    result = do_convert(
        torrent_path, bt_xltd_path, cfg_path, output_dir,
        allow_unverified=args.allow_unverified,
    )
    print(json.dumps(result, indent=2, ensure_ascii=False, default=str))
    
    report_path = output_dir / "conversion_report.json"
    report_path.write_text(json.dumps(result, indent=2, ensure_ascii=False, default=str))
    print(f"\n[OK] 转换报告: {report_path}")


if __name__ == "__main__":
    main()
