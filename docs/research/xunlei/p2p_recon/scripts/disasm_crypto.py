"""
反汇编 rc4_handler 和 XPF_AES* 函数,确认加密用法
"""
import pefile, capstone, re
from pathlib import Path

DLL = "/home/z/my-project/research/extracted/resource_1288_1304_unpacked/DownloadSDK.dll"
P2PBASE = "/home/z/my-project/research/extracted/resource_1288_1304_unpacked/P2PBase.dll"

import sys
sys.path.insert(0, '/home/z/my-project/scripts/p2p_recon')


def disasm_func(pe, md, image_base, mem, va, max_insns=60, label=""):
    rva = va - image_base
    if rva >= len(mem) or rva < 0:
        return
    code = mem[rva:rva+1500]
    insns = list(md.disasm(code, va))
    print(f"\n=== {label} @ {hex(va)} ({len(insns)} insns) ===")
    for i, ins in enumerate(insns[:max_insns]):
        line = f"0x{ins.address:x}: {ins.mnemonic} {ins.op_str}"
        if ins.mnemonic == 'lea' and 'rip' in ins.op_str:
            m = re.search(r'\[rip\s*\+\s*0x([0-9a-fA-F]+)\]', ins.op_str)
            if m:
                disp = int(m.group(1), 16)
                target_va = ins.address + ins.size + disp
                target_rva = target_va - image_base
                if 0 <= target_rva < len(mem):
                    end = mem.find(b'\x00', target_rva, target_rva+256)
                    if end > 0:
                        s = mem[target_rva:end].decode('ascii', errors='ignore')
                        if s.isprintable() and len(s) >= 3:
                            line += f'  ; "{s[:80]}"'
        m = re.search(r',\s*(0x[0-9a-fA-F]+|\d+)$', ins.op_str)
        if m:
            v = m.group(1)
            val = int(v, 16) if v.startswith('0x') else int(v)
            if 0 < val < 0x10000:
                line += f'  ; #{val}'
        print(f"  {line}")
        if ins.mnemonic == 'ret' and i > 5:
            break


def find_xpf_aes_exports():
    """找 P2PBase.dll 里的 XPF_AES* 导出"""
    pe = pefile.PE(P2PBASE, fast_load=True)
    pe.parse_data_directories()
    if not hasattr(pe, "DIRECTORY_ENTRY_EXPORT"):
        return {}
    exports = {}
    for exp in pe.DIRECTORY_ENTRY_EXPORT.symbols:
        if exp.name:
            n = exp.name.decode("utf-8", errors="ignore")
            if "AES" in n or "RC4" in n or "rc4" in n.lower():
                exports[n] = exp.address
    return exports


def main():
    # 1. P2PBase.dll 的 AES/RC4 导出
    print("=== P2PBase.dll AES/RC4 exports ===")
    aes_exports = find_xpf_aes_exports()
    for n, addr in sorted(aes_exports.items()):
        print(f"  {n}: {hex(addr)}")

    # 2. 反汇编 P2PBase.dll 的 XPF_AES* 函数
    pe = pefile.PE(P2PBASE, fast_load=True)
    pe.parse_data_directories()
    image_base = pe.OPTIONAL_HEADER.ImageBase
    mem = pe.get_memory_mapped_image()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    
    for name, rva in sorted(aes_exports.items()):
        if "ECB" in name:  # 只看 ECB 实现
            disasm_func(pe, md, image_base, mem, image_base + rva, max_insns=80, label=name)
    
    # 3. DownloadSDK.dll 里 rc4_handler 的 vtable
    print("\n\n=== DownloadSDK.dll rc4_handler 反汇编 ===")
    data = Path(DLL).read_bytes()
    pe2 = pefile.PE(DLL, fast_load=True)
    pe2.parse_data_directories()
    ib2 = pe2.OPTIONAL_HEADER.ImageBase
    mem2 = pe2.get_memory_mapped_image()
    
    # 找 .?AUrc4_handler@@ 字符串
    rtti = b".?AUrc4_handler@@"
    idx = data.find(rtti)
    print(f"rc4_handler RTTI at file offset: {hex(idx) if idx >= 0 else 'NOT FOUND'}")
    
    # 找所有引用 rc4_handler 字符串的位置
    rc4_strings = []
    a_pat = re.compile(rb'[\x20-\x7e]{4,}')
    for m in a_pat.finditer(data):
        s = m.group().decode('ascii', errors='ignore')
        if 'rc4_handler' in s or 'RC4' in s or 'rc4' in s.lower():
            if 4 < len(s) < 100:
                rc4_strings.append((m.start(), s))
    print(f"\nrc4 相关字符串:")
    for off, s in rc4_strings[:30]:
        print(f"  [{hex(off)}] {s}")


if __name__ == "__main__":
    main()
