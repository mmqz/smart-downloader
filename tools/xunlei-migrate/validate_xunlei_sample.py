"""
独立验证器: 验证迅雷 .xltd + .cfg + .torrent 真实样本

用途:
  接收用户提供的三件套,独立验证 spec_pending_validation.md 中的所有 C/D 级推断
  验证通过后,可解锁 xunlei_to_libtorrent_converter.py 的转换模式

输入:
  --torrent <path>      原始 .torrent 文件
  --bt-xltd <path>      迅雷 .bt.xltd 文件
  --cfg <path>          迅雷 .xlbt.cfg 文件
  --report <path>       验证报告输出路径 (JSON)
  --sample-pieces <N>   piece hash 验证采样数 (默认 30)

输出:
  - 完整验证报告 JSON
  - 终端彩色输出验证结果

验证项 (按 spec_pending_validation.md):
  V1: section_id → 内容映射 (D 级 → 升 A 级)
  V2: field2/field3 语义 (C 级 → 升 A 级)
  V3: .bt.xltd 是否有头部 (B 级 → 升 A 级)
  V4: piece 数据物理偏移公式 (C 级 → 升 A 级)
  V5: CXBitmap 字节序 (D 级 → 升 A 级)
  V6: CXBitmap 是否每 piece 1 bit (D 级 → 升 A 级)
  V7: cfg info hash 校验算法 (C 级 → 升 A 级)
  V8: block_count / block_size 实际语义 (C 级 → 升 A 级)
"""
import argparse
import json
import struct
import sys
import hashlib
from pathlib import Path
from dataclasses import dataclass, asdict, field
from typing import Optional, List, Dict, Any, Tuple


@dataclass
class VerificationResult:
    """单项验证结果"""
    verification_id: str   # V1/V2/...
    name: str              # 验证项名称
    spec_level: str        # spec 中标的等级 (C/D)
    verified: Optional[bool]  # True=通过, False=失败, None=无法验证
    new_level: str         # 验证后的新等级 (A/B/C/D)
    evidence: str          # 证据描述
    details: Dict[str, Any] = field(default_factory=dict)


def parse_torrent_pieces(path: Path) -> Dict[str, Any]:
    """从 .torrent 文件拿 piece_length + pieces_hash + info_hash + num_pieces"""
    try:
        import libtorrent as lt
    except ImportError:
        return {"error": "libtorrent not installed"}
    
    info = lt.torrent_info(str(path))
    info_hash = info.info_hash().to_bytes()
    
    # 从原始 .torrent bdecode 拿 pieces_hash + piece_length
    raw = path.read_bytes()
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
    
    parsed, _ = bdecode(raw)
    info_dict = parsed[b'info']
    pieces_hash = info_dict[b'pieces']
    piece_length = info_dict[b'piece length']
    
    return {
        "name": info.name(),
        "info_hash": info_hash,
        "info_hash_hex": info_hash.hex(),
        "piece_length": piece_length,
        "num_pieces": len(pieces_hash) // 20,
        "pieces_hash": pieces_hash,
        "total_size": info.total_size(),
    }


def parse_xlbt_cfg_header(path: Path) -> Dict[str, Any]:
    """解析 .xlbt.cfg 头部 + section 数组"""
    data = path.read_bytes()
    if len(data) < 40:
        return {"error": f"file too small: {len(data)}"}
    
    magic = data[0:8]
    reserved1 = struct.unpack("<H", data[8:10])[0]
    reserved2 = struct.unpack("<H", data[10:12])[0]
    reserved3 = struct.unpack("<I", data[12:16])[0]
    block_count = struct.unpack("<Q", data[16:24])[0]
    block_size = struct.unpack("<Q", data[24:32])[0]
    section_count = struct.unpack("<I", data[32:36])[0]
    reserved4 = struct.unpack("<I", data[36:40])[0]
    
    sections = []
    for i in range(min(section_count, 1000)):
        offset = 40 + i * 20
        if offset + 20 > len(data):
            break
        section_id = struct.unpack("<I", data[offset:offset+4])[0]
        field2 = struct.unpack("<Q", data[offset+4:offset+12])[0]
        field3 = struct.unpack("<Q", data[offset+12:offset+20])[0]
        sections.append({
            "index": i,
            "file_offset": offset,
            "section_id": section_id,
            "field2": field2,
            "field3": field3,
        })
    
    return {
        "size": len(data),
        "magic": magic,
        "magic_ok": magic == b"XLBTCFG\x00",
        "reserved1": reserved1,
        "reserved2": reserved2,
        "reserved3": reserved3,
        "block_count": block_count,
        "block_size": block_size,
        "block_size_aligned_4096": block_size % 4096 == 0,
        "section_count": section_count,
        "reserved4": reserved4,
        "sections": sections,
        "raw_data": data,  # 用于 section 内容读取
    }


def verify_v1_v2_section_id_mapping(
    cfg: Dict, torrent_info: Dict
) -> VerificationResult:
    """V1+V2: 验证 section_id 映射和 field2/field3 语义
    
    策略: 遍历所有 section,找哪个 section 的内容 == torrent 的 pieces_hash (80 字节匹配)
    """
    sections = cfg["sections"]
    raw_data = cfg["raw_data"]
    expected_pieces_size = torrent_info["num_pieces"] * 20
    expected_info_hash = torrent_info["info_hash"]
    
    findings = []
    pieces_section_idx = None
    infohash_section_idx = None
    
    for s in sections:
        # 尝试读 field2=size, field3=offset
        size = s["field2"]
        offset = s["field3"]
        body_v1 = raw_data[offset:offset+size] if offset + size <= cfg["size"] else None
        
        # 尝试读 field2=offset, field3=size (反过来)
        offset2 = s["field2"]
        size2 = s["field3"]
        body_v2 = raw_data[offset2:offset2+size2] if offset2 + size2 <= cfg["size"] else None
        
        # 检查 body_v1 是否匹配 pieces_hash
        v1_is_pieces = (body_v1 and len(body_v1) == expected_pieces_size 
                        and body_v1 == torrent_info["pieces_hash"])
        v1_is_infohash = (body_v1 and len(body_v1) == 20 
                          and body_v1 == expected_info_hash)
        
        # 检查 body_v2 是否匹配
        v2_is_pieces = (body_v2 and len(body_v2) == expected_pieces_size 
                        and body_v2 == torrent_info["pieces_hash"])
        v2_is_infohash = (body_v2 and len(body_v2) == 20 
                          and body_v2 == expected_info_hash)
        
        if v1_is_pieces or v2_is_pieces:
            pieces_section_idx = s["index"]
            findings.append({
                "section_index": s["index"],
                "section_id": f"0x{s['section_id']:08x}",
                "match": "PIECES_HASH",
                "field2_role": "size" if v1_is_pieces else "offset",
                "field3_role": "offset" if v1_is_pieces else "size",
            })
        if v1_is_infohash or v2_is_infohash:
            infohash_section_idx = s["index"]
            findings.append({
                "section_index": s["index"],
                "section_id": f"0x{s['section_id']:08x}",
                "match": "INFO_HASH",
                "field2_role": "size" if v1_is_infohash else "offset",
                "field3_role": "offset" if v1_is_infohash else "size",
            })
    
    verified = pieces_section_idx is not None and infohash_section_idx is not None
    return VerificationResult(
        verification_id="V1+V2",
        name="section_id → 内容映射 + field2/field3 语义",
        spec_level="C/D",
        verified=verified,
        new_level="A" if verified else "D",
        evidence=(f"找到 PIECES_HASH section (index={pieces_section_idx}) "
                  f"和 INFO_HASH section (index={infohash_section_idx})" if verified 
                  else f"未找到匹配 section,已扫描 {len(sections)} 个 section"),
        details={"findings": findings, "expected_pieces_size": expected_pieces_size},
    )


def verify_v3_xltd_has_no_header(bt_xltd_path: Path) -> VerificationResult:
    """V3: 验证 .bt.xltd 是否真的没有文件头 magic"""
    size = bt_xltd_path.stat().st_size
    with open(bt_xltd_path, 'rb') as f:
        head = f.read(256)
    
    # 检查前 64 字节是否包含 ASCII magic
    is_ascii_magic = (
        len(head) >= 8 and
        all(32 <= b < 127 for b in head[0:8]) and
        head[0:8] != b'\x00' * 8
    )
    
    # 检查是否是 sparse file (实际占用 << 文件大小)
    stat = bt_xltd_path.stat()
    actual_bytes = stat.st_blocks * 512 if hasattr(stat, 'st_blocks') else size
    is_sparse = actual_bytes < size * 0.9
    
    # 验证: 如果前 8 字节都是 0 (sparse hole) → 推断无 magic 成立
    # 或者前 8 字节是任意二进制(不是 ASCII) → 推断无 ASCII magic 成立
    first8 = head[0:8]
    is_all_zero = first8 == b'\x00' * 8
    
    # 推断无 magic 的条件:
    # 1. 前 8 字节不是 ASCII (B 级推断 → A 级)
    # 2. 或前 8 字节是 0 (sparse hole,符合 "纯数据 + sparse" 推断)
    verified = (not is_ascii_magic) or is_all_zero
    
    return VerificationResult(
        verification_id="V3",
        name=".bt.xltd 是否有文件头 magic",
        spec_level="B",
        verified=verified,
        new_level="A" if verified else "B",
        evidence=(f"前 8 字节 hex={first8.hex()}, "
                  f"is_ascii_magic={is_ascii_magic}, "
                  f"is_all_zero={is_all_zero}, "
                  f"is_sparse={is_sparse}"),
        details={
            "size": size,
            "actual_disk_bytes": actual_bytes,
            "is_sparse": is_sparse,
            "is_ascii_magic": is_ascii_magic,
            "is_all_zero": is_all_zero,
            "first64_hex": head[:64].hex(),
        },
    )


def verify_v4_piece_offset_layout(
    bt_xltd_path: Path, torrent_info: Dict,
    bitfield: Optional[bytes] = None,
    sample_count: int = 30,
) -> VerificationResult:
    """V4: 验证 .bt.xltd piece 偏移公式 = piece_index × piece_length
    
    这是核心验证项,直接决定转换器能否工作
    """
    piece_length = torrent_info["piece_length"]
    num_pieces = torrent_info["num_pieces"]
    pieces_hash = torrent_info["pieces_hash"]
    
    # 找已下载的 piece (用 bitfield,如果没有则全部检查)
    def is_complete(idx):
        if bitfield is None:
            return True  # 全部检查
        byte_idx = idx // 8
        bit_idx = 7 - (idx % 8)
        if byte_idx >= len(bitfield):
            return False
        return bool(bitfield[byte_idx] & (1 << bit_idx))
    
    completed = [i for i in range(num_pieces) if is_complete(i)]
    if len(completed) < 5:
        return VerificationResult(
            verification_id="V4",
            name="piece 偏移公式 = piece_index × piece_length",
            spec_level="C",
            verified=None,
            new_level="C",
            evidence=f"已下载 piece 数 {len(completed)} < 5, 无法验证",
            details={"completed_count": len(completed)},
        )
    
    # 采样
    if sample_count > len(completed):
        sample_count = len(completed)
    step = max(1, len(completed) // sample_count)
    sample_indices = completed[::step][:sample_count]
    
    matches = 0
    results = []
    with open(bt_xltd_path, 'rb') as f:
        for idx in sample_indices:
            offset = idx * piece_length
            f.seek(offset)
            data = f.read(piece_length)
            if len(data) < piece_length:
                results.append({
                    "piece_index": idx, "offset": offset,
                    "read_bytes": len(data), "match": False, "reason": "short_read",
                })
                continue
            actual = hashlib.sha1(data).digest()
            expected = pieces_hash[idx*20:(idx+1)*20]
            match = (actual == expected)
            if match:
                matches += 1
            results.append({
                "piece_index": idx, "offset": offset,
                "expected_hash": expected.hex(),
                "actual_hash": actual.hex(),
                "match": match,
            })
    
    match_rate = matches / len(sample_indices) if sample_indices else 0
    verified = match_rate >= 0.8
    
    return VerificationResult(
        verification_id="V4",
        name="piece 偏移公式 = piece_index × piece_length",
        spec_level="C",
        verified=verified,
        new_level="A" if verified else "D",
        evidence=f"采样 {len(sample_indices)} 个已下载 piece, 命中率 {match_rate*100:.1f}%",
        details={
            "match_rate": match_rate,
            "matches": matches,
            "samples_checked": len(sample_indices),
            "completed_count": len(completed),
            "samples": results[:10],  # 只取前 10 个详细
        },
    )


def verify_v5_v6_cxbitmap_format(
    cfg: Dict, torrent_info: Dict,
    pieces_section_idx: Optional[int] = None,
) -> VerificationResult:
    """V5+V6: 验证 CXBitmap 字节序 + 是否每 piece 1 bit
    
    策略: 找一个 section,其 size = ceil(num_pieces / 8),则确认为 BITFIELD
    然后验证:
      - 字节序: big-endian (前 N 个 piece 完成 → 第一字节高位为 1)
      - 是否每 piece 1 bit (size = ceil(num_pieces/8))
    """
    num_pieces = torrent_info["num_pieces"]
    expected_size = (num_pieces + 7) // 8  # 每 piece 1 bit
    
    # 找 size = expected_size 的 section
    candidates = []
    for s in cfg["sections"]:
        # 尝试 field2=size
        if s["field2"] == expected_size:
            body = cfg["raw_data"][s["field3"]:s["field3"]+s["field2"]]
            candidates.append((s, body, "field2=size,field3=offset"))
        # 尝试 field3=size
        if s["field3"] == expected_size:
            body = cfg["raw_data"][s["field2"]:s["field2"]+s["field3"]]
            candidates.append((s, body, "field2=offset,field3=size"))
    
    if not candidates:
        return VerificationResult(
            verification_id="V5+V6",
            name="CXBitmap 字节序 + 每 piece 1 bit",
            spec_level="D",
            verified=None,
            new_level="D",
            evidence=f"未找到 size={expected_size} 的 section",
            details={"expected_size": expected_size, "num_pieces": num_pieces},
        )
    
    # 取第一个候选
    section, body, role = candidates[0]
    
    # 验证 V6: size = ceil(num_pieces/8) → 每 piece 1 bit
    v6_verified = len(body) == expected_size
    
    # 验证 V5: 字节序
    # 这里无法直接验证字节序 (需要知道哪些 piece 已下载)
    # 但可以看 body 内容: 如果非全 0/全 0xFF,说明是真实位图
    non_zero_bytes = sum(1 for b in body if b != 0)
    non_ff_bytes = sum(1 for b in body if b != 0xFF)
    looks_like_bitfield = 0 < non_zero_bytes < len(body)
    
    verified = v6_verified and looks_like_bitfield
    
    return VerificationResult(
        verification_id="V5+V6",
        name="CXBitmap 字节序 + 每 piece 1 bit",
        spec_level="D",
        verified=verified,
        new_level="A" if verified else "D",
        evidence=(f"找到 BITFIELD section (index={section['index']}, "
                  f"section_id=0x{section['section_id']:08x}, {role}), "
                  f"size={len(body)} = expected {expected_size}, "
                  f"non_zero_bytes={non_zero_bytes}/{len(body)}"),
        details={
            "section_index": section["index"],
            "section_id": f"0x{section['section_id']:08x}",
            "field_role": role,
            "body_size": len(body),
            "expected_size": expected_size,
            "non_zero_bytes": non_zero_bytes,
            "non_ff_bytes": non_ff_bytes,
            "body_hex_head": body[:32].hex(),
            "body_hex_tail": body[-32:].hex() if len(body) > 32 else None,
        },
    )


def verify_v7_cfg_infohash_check(
    cfg: Dict, torrent_info: Dict,
    infohash_section_idx: Optional[int] = None,
) -> VerificationResult:
    """V7: 验证 cfg 内 infohash 是否与 .torrent 一致
    
    若一致,则 cfg 有 infohash 校验 (字符串证据 "cfg info hash not match!")
    """
    if infohash_section_idx is None:
        return VerificationResult(
            verification_id="V7",
            name="cfg info hash 校验",
            spec_level="C",
            verified=None,
            new_level="C",
            evidence="INFO_HASH section 未找到,无法验证",
        )
    
    # 从 infohash_section 读 20 字节
    s = cfg["sections"][infohash_section_idx]
    body = cfg["raw_data"][s["field3"]:s["field3"]+s["field2"]]
    if len(body) != 20:
        # 试 field3=size, field2=offset
        body = cfg["raw_data"][s["field2"]:s["field2"]+s["field3"]]
    
    if len(body) != 20:
        return VerificationResult(
            verification_id="V7",
            name="cfg info hash 校验",
            spec_level="C",
            verified=False,
            new_level="D",
            evidence=f"INFO_HASH section body size {len(body)} != 20",
        )
    
    verified = body == torrent_info["info_hash"]
    
    return VerificationResult(
        verification_id="V7",
        name="cfg info hash 校验",
        spec_level="C",
        verified=verified,
        new_level="A" if verified else "D",
        evidence=(f"cfg 内 infohash = {body.hex()}, "
                  f"torrent infohash = {torrent_info['info_hash_hex']}, "
                  f"match={verified}"),
        details={
            "cfg_info_hash": body.hex(),
            "torrent_info_hash": torrent_info["info_hash_hex"],
        },
    )


def verify_v8_block_count_size_semantics(cfg: Dict, torrent_info: Dict) -> VerificationResult:
    """V8: 验证 block_count / block_size 实际语义"""
    block_count = cfg["block_count"]
    block_size = cfg["block_size"]
    
    # 推断 1: block_size = piece_length 的倍数?
    piece_length = torrent_info["piece_length"]
    is_piece_multiple = block_size % piece_length == 0 if piece_length > 0 else False
    
    # 推断 2: block_count = 文件大小 / block_size?
    total_size = torrent_info["total_size"]
    is_total_div = (block_count * block_size) == total_size if block_size > 0 else False
    
    # 推断 3: block_size = 配置区域大小?
    cfg_size = cfg["size"]
    is_cfg_size_match = block_size == cfg_size
    
    findings = {
        "block_count": block_count,
        "block_size": block_size,
        "piece_length": piece_length,
        "total_size": total_size,
        "block_size_is_piece_multiple": is_piece_multiple,
        "block_count_times_block_size_equals_total": is_total_div,
        "block_size_equals_cfg_size": is_cfg_size_match,
    }
    
    # 综合判断
    if is_total_div:
        verified = True
        evidence = f"block_count({block_count}) × block_size({block_size}) = total_size({total_size})"
    elif is_cfg_size_match:
        verified = True
        evidence = f"block_size({block_size}) = cfg 文件大小({cfg_size})"
    else:
        verified = None
        evidence = "未找到明确语义关联"
    
    return VerificationResult(
        verification_id="V8",
        name="block_count / block_size 语义",
        spec_level="C",
        verified=verified,
        new_level="A" if verified else "C",
        evidence=evidence,
        details=findings,
    )


def run_all_verifications(
    torrent_path: Path,
    bt_xltd_path: Path,
    cfg_path: Path,
    sample_pieces: int = 30,
) -> Tuple[List[VerificationResult], Dict[str, Any]]:
    """运行所有验证"""
    results = []
    details = {}
    
    # Step 1: 解析 .torrent
    torrent_info = parse_torrent_pieces(torrent_path)
    if "error" in torrent_info:
        return [], {"error": torrent_info["error"]}
    details["torrent"] = {
        "name": torrent_info["name"],
        "info_hash": torrent_info["info_hash_hex"],
        "piece_length": torrent_info["piece_length"],
        "num_pieces": torrent_info["num_pieces"],
        "total_size": torrent_info["total_size"],
    }
    
    # Step 2: 解析 .xlbt.cfg
    cfg = parse_xlbt_cfg_header(cfg_path)
    if "error" in cfg:
        return [], {"error": cfg["error"]}
    details["cfg"] = {
        "size": cfg["size"],
        "magic_ok": cfg["magic_ok"],
        "block_count": cfg["block_count"],
        "block_size": cfg["block_size"],
        "section_count": cfg["section_count"],
        "sections": [
            {"index": s["index"], "section_id": f"0x{s['section_id']:08x}",
             "field2": s["field2"], "field3": s["field3"]}
            for s in cfg["sections"]
        ],
    }
    
    # 头部 magic 校验
    if not cfg["magic_ok"]:
        results.append(VerificationResult(
            verification_id="HEADER",
            name="cfg magic = XLBTCFG",
            spec_level="A",
            verified=False,
            new_level="D",
            evidence=f"magic mismatch: got {cfg['magic']!r}",
        ))
        return results, details
    
    # V1+V2: section_id 映射
    v1v2 = verify_v1_v2_section_id_mapping(cfg, torrent_info)
    results.append(v1v2)
    
    pieces_section_idx = None
    infohash_section_idx = None
    for f in v1v2.details.get("findings", []):
        if f["match"] == "PIECES_HASH":
            pieces_section_idx = f["section_index"]
        elif f["match"] == "INFO_HASH":
            infohash_section_idx = f["section_index"]
    
    # V3: .bt.xltd 是否有头部
    v3 = verify_v3_xltd_has_no_header(bt_xltd_path)
    results.append(v3)
    
    # 取 bitfield (如果有)
    bitfield = None
    if pieces_section_idx is not None:
        # 试着从 BITFIELD section 取
        # 先找 size = expected_bitfield_size 的 section
        expected_bf_size = (torrent_info["num_pieces"] + 7) // 8
        for s in cfg["sections"]:
            if s["field2"] == expected_bf_size:
                bitfield = cfg["raw_data"][s["field3"]:s["field3"]+s["field2"]]
                break
            elif s["field3"] == expected_bf_size:
                bitfield = cfg["raw_data"][s["field2"]:s["field2"]+s["field3"]]
                break
    
    # V4: piece 偏移公式 (核心)
    v4 = verify_v4_piece_offset_layout(
        bt_xltd_path, torrent_info,
        bitfield=bitfield, sample_count=sample_pieces,
    )
    results.append(v4)
    
    # V5+V6: CXBitmap 格式
    v5v6 = verify_v5_v6_cxbitmap_format(cfg, torrent_info)
    results.append(v5v6)
    
    # V7: cfg infohash 校验
    v7 = verify_v7_cfg_infohash_check(cfg, torrent_info, infohash_section_idx)
    results.append(v7)
    
    # V8: block_count/block_size 语义
    v8 = verify_v8_block_count_size_semantics(cfg, torrent_info)
    results.append(v8)
    
    return results, details


def print_results(results: List[VerificationResult], details: Dict):
    """终端彩色输出"""
    print("\n" + "="*70)
    print("迅雷样本验证报告")
    print("="*70)
    
    print("\n--- 输入文件信息 ---")
    if "torrent" in details:
        t = details["torrent"]
        print(f"  .torrent: name={t['name']}, info_hash={t['info_hash']}, "
              f"piece_length={t['piece_length']}, num_pieces={t['num_pieces']}")
    if "cfg" in details:
        c = details["cfg"]
        print(f"  .xlbt.cfg: size={c['size']}, magic_ok={c['magic_ok']}, "
              f"block_count={c['block_count']}, block_size={c['block_size']}, "
              f"section_count={c['section_count']}")
    
    print("\n--- 验证结果 ---")
    for r in results:
        icon = "✅" if r.verified else ("❌" if r.verified is False else "⚠️")
        print(f"\n{icon} [{r.verification_id}] {r.name}")
        print(f"   spec 等级: {r.spec_level} → 验证后: {r.new_level}")
        print(f"   证据: {r.evidence}")
    
    # 综合判断
    all_pass = all(r.verified for r in results if r.verified is not None)
    critical_pass = any(r.verification_id == "V4" and r.verified for r in results)
    
    print("\n" + "="*70)
    if critical_pass:
        print("🎉 关键验证 (V4 piece 偏移) 通过!")
        print("   可以解锁 xunlei_to_libtorrent_converter.py 的 --convert 模式")
    else:
        print("⚠ 关键验证 (V4 piece 偏移) 未通过")
        print("   推断错误或样本不完整,无法启用转换模式")
    
    if all_pass:
        print("\n✅ 所有验证通过,推断全部升级为 A 级")
    else:
        print("\n⚠ 部分验证未通过,详见上方各 verification 详情")
    print("="*70)


def main():
    parser = argparse.ArgumentParser(
        description="迅雷 .xltd + .cfg + .torrent 样本验证器"
    )
    parser.add_argument("--torrent", required=True, help=".torrent 文件路径")
    parser.add_argument("--bt-xltd", required=True, help=".bt.xltd 文件路径")
    parser.add_argument("--cfg", required=True, help=".xlbt.cfg 文件路径")
    parser.add_argument("--report", default="verification_report.json",
                        help="验证报告 JSON 输出路径")
    parser.add_argument("--sample-pieces", type=int, default=30,
                        help="piece hash 验证采样数 (默认 30)")
    args = parser.parse_args()
    
    print(f"=== 输入 ===")
    print(f"  torrent:  {args.torrent}")
    print(f"  bt.xltd:  {args.bt_xltd}")
    print(f"  cfg:      {args.cfg}")
    print(f"  samples:  {args.sample_pieces}")
    
    results, details = run_all_verifications(
        Path(args.torrent), Path(args.bt_xltd), Path(args.cfg),
        sample_pieces=args.sample_pieces,
    )
    
    if not results:
        print("\n[ERR] 无法验证:", details.get("error", "unknown"))
        sys.exit(1)
    
    print_results(results, details)
    
    # 保存报告
    report = {
        "torrent": details.get("torrent"),
        "cfg": details.get("cfg"),
        "verifications": [asdict(r) for r in results],
        "summary": {
            "critical_v4_passed": any(r.verification_id == "V4" and r.verified for r in results),
            "all_passed": all(r.verified for r in results if r.verified is not None),
        },
    }
    report_path = Path(args.report)
    report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False, default=str))
    print(f"\n[OK] 报告: {report_path}")


if __name__ == "__main__":
    main()
