"""
P0: 反汇编 XBTPackage 25 个类的 vtable
找 BT message id / 私有 ext_id / 加密特征 / 私有载荷格式
"""
import pefile, capstone, re, json
from pathlib import Path

DLL = "/home/z/my-project/research/extracted/resource_1288_1304_unpacked/DownloadSDK.dll"
OUT = Path("/home/z/my-project/research/p2p_recon")
OUT.mkdir(exist_ok=True, parents=True)

TARGET_CLASSES = [
    "XBTPackageAllowedFast", "XBTPackageBase", "XBTPackageBitField",
    "XBTPackageCancel", "XBTPackageChoke", "XBTPackageExtHandshake",
    "XBTPackageHandshake", "XBTPackageHave", "XBTPackageHaveAll",
    "XBTPackageHaveNone", "XBTPackageInterest", "XBTPackageKeepAlive",
    "XBTPackageMSE", "XBTPackageMetadata", "XBTPackageNotInterest",
    "XBTPackagePEX", "XBTPackagePort", "XBTPackagePunchingHole",
    "XBTPackageRejectRequest", "XBTPackageRequest", "XBTPackageSuggestPiece",
    "XBTPackageUnChoke",
]


def find_vtables(data, pe, image_base):
    results = {}
    for cls in TARGET_CLASSES:
        rtti_str = f".?AV{cls}@@".encode()
        idx = data.find(rtti_str)
        if idx < 0:
            continue
        td_file_off = idx - 16
        try:
            td_rva = pe.get_rva_from_offset(td_file_off)
        except Exception:
            continue
        if td_rva is None:
            continue
        td_rva_bytes = td_rva.to_bytes(4, 'little')
        vtables = []
        for sec in pe.sections:
            sec_name = sec.Name.decode(errors='ignore').rstrip('\x00')
            if not sec_name.startswith('.rdata'):
                continue
            rdata_start = sec.PointerToRawData
            rdata_end = rdata_start + sec.SizeOfRawData
            rdata = data[rdata_start:rdata_end]
            p = 0
            while True:
                i = rdata.find(td_rva_bytes, p)
                if i < 0:
                    break
                p = i + 1
                col_off_in_rdata = i - 12
                if col_off_in_rdata < 0:
                    continue
                col_file_off = rdata_start + col_off_in_rdata
                col_rva = pe.get_rva_from_offset(col_file_off)
                if col_rva is None:
                    continue
                col_va = image_base + col_rva
                col_va_bytes = col_va.to_bytes(8, 'little')
                vtable_ptr_idx = data.find(col_va_bytes)
                if vtable_ptr_idx >= 0:
                    vtable_file_off = vtable_ptr_idx + 8
                    vtable_rva = pe.get_rva_from_offset(vtable_file_off)
                    if vtable_rva:
                        vtables.append(image_base + vtable_rva)
        if vtables:
            seen = set()
            uniq = []
            for v in vtables:
                if v not in seen:
                    seen.add(v)
                    uniq.append(v)
            results[cls] = uniq
    return results


def disasm_method(mem, md, image_base, va, max_insns=80):
    rva = va - image_base
    if rva >= len(mem) or rva < 0:
        return None
    code = mem[rva:rva+2000]
    insns = list(md.disasm(code, va))
    if not insns:
        return None
    result = {
        "va": hex(va),
        "insns_count": 0,
        "string_refs": [],
        "immediates": [],
        "calls": [],
        "mem_writes": [],
        "mem_reads": [],
    }
    for i, ins in enumerate(insns):
        if i > max_insns:
            break
        result["insns_count"] += 1
        if ins.mnemonic == "lea" and "rip" in ins.op_str:
            m = re.search(r"\[rip\s*\+\s*0x([0-9a-fA-F]+)\]", ins.op_str)
            if m:
                disp = int(m.group(1), 16)
                target_va = ins.address + ins.size + disp
                target_rva = target_va - image_base
                if 0 <= target_rva < len(mem):
                    end = mem.find(b"\x00", target_rva, target_rva + 256)
                    if end > 0:
                        s = mem[target_rva:end].decode("ascii", errors="ignore")
                        if s.isprintable() and len(s) >= 3:
                            result["string_refs"].append({"addr": hex(target_va), "value": s[:200]})
        elif ins.mnemonic in ["mov", "cmp", "movzx", "movsxd", "xor"]:
            m = re.match(r"(\w+),\s*(0x[0-9a-fA-F]+|\d+)$", ins.op_str)
            if m:
                val_str = m.group(2)
                val = int(val_str, 16) if val_str.startswith('0x') else int(val_str)
                if 0 < val < 0x10000:
                    result["immediates"].append({"reg": m.group(1), "val": val, "val_hex": hex(val)})
        elif ins.mnemonic == "call":
            result["calls"].append({"insn": f"0x{ins.address:x}: call {ins.op_str}", "target": ins.op_str})
        m_w = re.match(r"mov\s+(?:dword|word|qword|byte) ptr \[(\w+)\s*\+\s*(0x[0-9a-fA-F]+|\d+)\],\s*(\w+)", ins.op_str)
        if m_w:
            result["mem_writes"].append({"base": m_w.group(1), "offset": m_w.group(2), "src": m_w.group(3)})
        m_r = re.match(r"(\w+),\s*(?:dword|word|qword|byte) ptr \[(\w+)\s*\+\s*(0x[0-9a-fA-F]+|\d+)\]", ins.op_str)
        if m_r:
            result["mem_reads"].append({"dst": m_r.group(1), "base": m_r.group(2), "offset": m_r.group(3)})
        if ins.mnemonic == "ret" and i > 5:
            break
    return result


def main():
    pe = pefile.PE(DLL, fast_load=True)
    pe.parse_data_directories()
    image_base = pe.OPTIONAL_HEADER.ImageBase
    mem = pe.get_memory_mapped_image()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    data = Path(DLL).read_bytes()

    print(f"[*] finding vtables for {len(TARGET_CLASSES)} XBTPackage classes...")
    vtables_map = find_vtables(data, pe, image_base)
    print(f"[*] found {len(vtables_map)} classes")
    for cls, vts in vtables_map.items():
        print(f"  {cls}: {len(vts)} vtable(s)")

    results = {}
    for cls, vts in vtables_map.items():
        results[cls] = []
        for vt_va in vts[:2]:
            vt_info = {"vtable_va": hex(vt_va), "methods": []}
            rva = vt_va - image_base
            for i in range(12):
                ptr = int.from_bytes(mem[rva+i*8:rva+i*8+8], 'little')
                if ptr == 0 or ptr < image_base or ptr > image_base + len(mem):
                    break
                method = disasm_method(mem, md, image_base, ptr, max_insns=50)
                if method:
                    method["index"] = i
                    vt_info["methods"].append(method)
            results[cls].append(vt_info)

    print(f"\n[*] analyzing protocol constants...")

    # 1. 标准 BT message_id (0-19)
    print("\n=== BT Message ID 候选 (0-19 范围) ===")
    for cls, vts in results.items():
        for vt in vts:
            for m in vt["methods"]:
                msg_candidates = [im for im in m["immediates"] if 0 < im["val"] <= 19]
                if msg_candidates:
                    print(f"  {cls}::vtable[{m['index']}] @ {m['va']}: msg_id candidates = {[(im['reg'], im['val']) for im in msg_candidates[:3]]}")

    # 2. 私有 ext_id (>20)
    print("\n=== 私有扩展 ID 候选 (>20 范围, 重点 PunchingHole/SuggestPiece) ===")
    for cls in ["XBTPackagePunchingHole", "XBTPackageSuggestPiece", "XBTPackageExtHandshake", "XBTPackageMetadata", "XBTPackagePEX"]:
        if cls not in results:
            continue
        for vt in results[cls]:
            for m in vt["methods"]:
                ext_candidates = [im for im in m["immediates"] if 20 < im["val"] < 256]
                if ext_candidates:
                    print(f"  {cls}::vtable[{m['index']}] @ {m['va']}:")
                    for im in ext_candidates[:5]:
                        print(f"    {im['reg']} = {im['val_hex']} ({im['val']})")

    # 3. 加密特征
    print("\n=== XBTPackageMSE 加密特征 ===")
    if "XBTPackageMSE" in results:
        for vt in results["XBTPackageMSE"]:
            for m in vt["methods"]:
                # RC4: 256 字节状态; AES: 16 字节块; SHA1 init constants
                crypto_consts = [im for im in m["immediates"] 
                                 if im["val"] in [256, 0x100, 16, 24, 32, 0x67452301, 0xefcdab89, 
                                                    0x98badcfe, 0x10325476, 0xc3d2e1f0]
                                 or (0x100 <= im["val"] <= 0x1000)]
                if crypto_consts:
                    print(f"  MSE::vtable[{m['index']}] @ {m['va']}:")
                    for im in crypto_consts[:5]:
                        print(f"    {im['reg']} = {im['val_hex']} ({im['val']})")
                if m["string_refs"]:
                    for s in m["string_refs"]:
                        sval = s["value"]
                        if any(kw in sval.lower() for kw in ["rc4", "aes", "sha1", "md5", "mse", 
                                                                "encrypt", "decrypt", "key", "cipher"]):
                            print(f"    str: [{s['addr']}] {sval[:120]}")

    # 4. 所有字符串引用
    print("\n=== 所有 XBTPackage 类的协议相关字符串引用 ===")
    for cls, vts in results.items():
        for vt in vts:
            for m in vt["methods"]:
                if m["string_refs"]:
                    for s in m["string_refs"]:
                        sval = s["value"]
                        if any(kw in sval for kw in ["msg_type", "message_type", "ext_id", "ext_name",
                                                      "handshake", "metadata", "ut_metadata", "ut_pex",
                                                      "punch", "suggest", "RC4", "AES", "MSE",
                                                      "encrypt", "decrypt", "key", "cipher",
                                                      "XL", "p2sp", "thunder", "xl_"]):
                            print(f"  {cls}::vtable[{m['index']}]: [{s['addr']}] {sval[:120]}")

    # 5. 内存写模式 (struct 字段布局)
    print("\n=== PunchingHole / SuggestPiece 结构体字段访问 ===")
    for cls in ["XBTPackagePunchingHole", "XBTPackageSuggestPiece"]:
        if cls not in results:
            continue
        for vt in results[cls]:
            for m in vt["methods"][:5]:
                if m["mem_writes"] or m["mem_reads"]:
                    print(f"  {cls}::vtable[{m['index']}] @ {m['va']}:")
                    for w in m["mem_writes"][:5]:
                        print(f"    WRITE [{w['base']}+{w['offset']}] = {w['src']}")
                    for r in m["mem_reads"][:5]:
                        print(f"    READ {r['dst']} = [{r['base']}+{r['offset']}]")

    (OUT / "xbtpackage_vtables.json").write_text(
        json.dumps(results, indent=2, ensure_ascii=False, default=str)
    )
    print(f"\n[OK] full results in {OUT}/xbtpackage_vtables.json")


if __name__ == "__main__":
    main()
