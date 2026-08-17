"""
逆向 PHub / SHub 包格式 - 从 DownloadSDK.dll 反汇编
重点找:
1. CmdPHubQueryRes - 查询资源 (peer 列表)
2. CmdPHubInsertRC - 上报资源 (告知服务器我有这个文件)
3. CmdSHubQueryBTFileIndex - BT 文件索引查询
4. 看包构造: header 格式 + cmd_id + payload
"""
import pefile, capstone, re
from pathlib import Path
import json

DLL = "/home/z/my-project/research/extracted/resource_1288_1304_unpacked/DownloadSDK.dll"
OUT = Path("/home/z/my-project/research/p2p_recon")
OUT.mkdir(exist_ok=True, parents=True)

# 28 个 PHUB/SHUB 协议常量字符串
# PHUB__GATEWAY__COMMID__QUERY_RES_REQ 等
PROTO_CONSTS = [
    "PHUB__PING__COMMID__PING_REQ",
    "PHUB__PING__COMMID__PING_RESP",
    "PHUB__PING__COMMID__LOGOUT",
    "PHUB__GATEWAY__COMMID__QUERY_RES_REQ",
    "PHUB__GATEWAY__COMMID__QUERY_RES_RESP",
    "PHUB__GATEWAY__COMMID__REPORT_RCS_REQ",
    "PHUB__GATEWAY__COMMID__REPORT_RCS_RESP",
    "PHUB__GATEWAY__COMMID__DELETE_RCS_REQ",
    "PHUB__GATEWAY__COMMID__DELETE_RCS_RESP",
    "PHUB__GATEWAY__COMMID__INVALID_PEER_REQ",
    "PHUB__GATEWAY__COMMID__RES_NEED_REPORT_REQ",
    "PHUB__GATEWAY__COMMID__RES_NEED_REPORT_RESP",
    # Cmd 类
    "CmdPHubQueryRes",
    "CmdPHubInsertRC",
    "CmdPHubDeleteRC",
    "CmdPHubInvalidPeer",
    "CmdPHubIsRCOnline",
    "CmdPHubNeedSyncCidStore",
    "CmdPHubGetCidStore",
    "CmdPHubReportCidStore",
    "CmdPHubReportRCList",
    "CmdSHubQueryBTFileIndex",
    "CmdSHubQueryTorrentFile",
    "CmdSHubInsertBCID",
    "CmdSHubInsertBTResource",
    "CmdSHubInsertServerRes",
    "CmdSHubQueryEmuleInfo",
    "CmdSHubQueryServerRes",
    "CmdSHubQueryUrlInfo",
    "CmdSHubReportCorrection",
    "CmdSHubReportResQuality",
    "CmdSHubReportURLChange",
]


def find_str_va(data, pe, image_base, target):
    """找字符串的 VA"""
    idx = data.find(target.encode())
    if idx < 0:
        return None
    rva = pe.get_rva_from_offset(idx)
    return image_base + rva if rva else None


def find_lea_refs(text_data, text_base_va, target_va):
    """找所有 lea 指令引用 target_va 的位置"""
    refs = []
    for opcode in [b"\x48\x8d\x05", b"\x48\x8d\x0d", b"\x48\x8d\x15",
                   b"\x48\x8d\x1d", b"\x48\x8d\x25", b"\x48\x8d\x2d",
                   b"\x48\x8d\x35", b"\x48\x8d\x3d",
                   b"\x4c\x8d\x05", b"\x4c\x8d\x0d", b"\x4c\x8d\x15",
                   b"\x4c\x8d\x1d", b"\x4c\x8d\x25", b"\x4c\x8d\x2d",
                   b"\x4c\x8d\x35", b"\x4c\x8d\x3d"]:
        pos = 0
        while True:
            i = text_data.find(opcode, pos)
            if i < 0: break
            pos = i + 1
            if i + 7 > len(text_data): continue
            disp = int.from_bytes(text_data[i+3:i+7], 'little', signed=True)
            ins_end = text_base_va + i + 7
            if ins_end + disp == target_va:
                refs.append(text_base_va + i)
                break  # 只要第一个
    return refs


def disasm_around(data, pe, image_base, mem, md, ref_va, before=200, length=2000):
    """反汇编 ref 附近的代码"""
    ref_off = pe.get_offset_from_rva(ref_va - image_base)
    code = data[max(0, ref_off-before):ref_off+length]
    insns = list(md.disasm(code, ref_va - before))
    return insns


def main():
    pe = pefile.PE(DLL, fast_load=True)
    pe.parse_data_directories()
    image_base = pe.OPTIONAL_HEADER.ImageBase
    mem = pe.get_memory_mapped_image()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    data = Path(DLL).read_bytes()
    
    # 找 .text 节
    text_sec = None
    for sec in pe.sections:
        if sec.Name.decode(errors='ignore').startswith('.text'):
            text_sec = sec
            break
    text_data = text_sec.get_data()
    text_base_va = image_base + text_sec.VirtualAddress
    
    # 对每个协议常量/类名,找它的引用位置
    results = {}
    for target in PROTO_CONSTS:
        str_va = find_str_va(data, pe, image_base, target)
        if not str_va:
            results[target] = {"found": False}
            continue
        refs = find_lea_refs(text_data, text_base_va, str_va)
        if not refs:
            results[target] = {"found": True, "str_va": hex(str_va), "refs": []}
            continue
        
        # 反汇编第一个 ref 附近代码
        ref_va = refs[0]
        insns = disasm_around(data, pe, image_base, mem, md, ref_va, before=50, length=3000)
        
        # 提取关键信息
        result = {
            "found": True,
            "str_va": hex(str_va),
            "ref_va": hex(ref_va),
            "string_refs": [],
            "immediates": [],
            "calls": [],
        }
        for ins in insns[:60]:
            if ins.address > ref_va + 1500:
                break
            if ins.address < ref_va - 30:
                continue
            # 字符串引用
            if ins.mnemonic == 'lea' and 'rip' in ins.op_str:
                m = re.search(r'\[rip\s*\+\s*0x([0-9a-fA-F]+)\]', ins.op_str)
                if m:
                    disp = int(m.group(1), 16)
                    t_va = ins.address + ins.size + disp
                    t_rva = t_va - image_base
                    if 0 <= t_rva < len(mem):
                        end = mem.find(b'\x00', t_rva, t_rva+256)
                        if end > 0:
                            s = mem[t_rva:end].decode('ascii', errors='ignore')
                            if s.isprintable() and len(s) >= 3:
                                result["string_refs"].append({
                                    "addr": hex(t_va),
                                    "value": s[:200],
                                    "at": hex(ins.address),
                                })
            # 立即数
            m = re.search(r',\s*(0x[0-9a-fA-F]+|\d+)$', ins.op_str)
            if m:
                v = m.group(1)
                val = int(v, 16) if v.startswith('0x') else int(v)
                if 0 < val < 0x10000:
                    result["immediates"].append({
                        "reg": ins.op_str.split(',')[0].strip(),
                        "val": val,
                        "val_hex": hex(val),
                        "at": hex(ins.address),
                    })
            # 调用
            if ins.mnemonic == 'call':
                result["calls"].append({
                    "target": ins.op_str,
                    "at": hex(ins.address),
                })
        
        results[target] = result
        print(f"\n[{target}] str@{hex(str_va)}, ref@{hex(ref_va)}")
        print(f"  string_refs ({len(result['string_refs'])}):")
        for s in result["string_refs"][:5]:
            print(f"    [{s['at']}] {s['value']}")
        print(f"  immediates ({len(result['immediates'])}):")
        for im in result["immediates"][:8]:
            print(f"    [{im['at']}] {im['reg']} = {im['val_hex']} ({im['val']})")
        print(f"  calls ({len(result['calls'])}):")
        for c in result["calls"][:5]:
            print(f"    [{c['at']}] {c['target']}")
    
    (OUT / "phub_shub_cmd_analysis.json").write_text(
        json.dumps(results, indent=2, ensure_ascii=False, default=str)
    )
    print(f"\n[OK] results in {OUT}/phub_shub_cmd_analysis.json")


if __name__ == "__main__":
    main()
