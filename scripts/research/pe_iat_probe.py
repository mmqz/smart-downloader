#!/usr/bin/env python3
"""pe_iat_probe.py - parse the PE import table of an exported module image,
find IAT slots for names matching a pattern (e.g. XPF_ParamStreamWrite*),
read the RUNTIME function address from the dump's memory (IAT slots are
patched by the loader), and print them for disassembly.

Usage:
    python pe_iat_probe.py scripts/research/captures/modules/Http.dll.bin \
        --dump <dump.dmp> --base 0x7ffd4fb10000 --pattern XPF_ParamStreamWrite
"""
import argparse
import struct


def pe_imports(blob):
    """Yield (module_name, [(func_name_or_ord, iat_rva), ...])."""
    mz = blob.find(b'MZ')  # 导出可能带内存段前缀
    if mz < 0:
        raise ValueError('not PE')
    e_lfanew = struct.unpack_from('<I', blob, mz + 0x3C)[0]
    e_lfanew += mz
    if blob[e_lfanew:e_lfanew + 4] != b'PE\0\0':
        raise ValueError('no PE signature')
    opt = e_lfanew + 24
    magic = struct.unpack_from('<H', blob, opt)[0]
    dd_off = opt + (112 if magic == 0x20B else 96)  # PE32+ data dirs at opt+112
    imp_rva, imp_size = struct.unpack_from('<II', blob, dd_off + 1 * 8)  # dir[1] = import
    if not imp_rva:
        return
    sections = []
    nsec = struct.unpack_from('<H', blob, e_lfanew + 6)[0]
    sec_off = e_lfanew + 24 + (240 if magic == 0x20B else 224)
    for i in range(nsec):
        name = blob[sec_off + i * 40:sec_off + i * 40 + 8].rstrip(b'\0')
        vs, va, rs, ro = struct.unpack_from('<IIII', blob, sec_off + i * 40 + 8)
        sections.append((name, va, vs, ro, rs))

    def rva2off(rva):
        # 导出镜像带段前缀: blob[0]=其他模块残留, blob[mz]=模块 image base(RVA 0)
        return mz + rva

    off = rva2off(imp_rva)
    while off is not None:
        oft_rva, ts, fwd, name_rva, fth_rva = struct.unpack_from('<IIIII', blob, off)
        if not (oft_rva or fth_rva):
            break
        dll_name = ''
        no = rva2off(name_rva)
        if no is not None:
            end = blob.find(b'\0', no)
            dll_name = blob[no:end].decode('ascii', 'ignore')
        entries = []
        o, f = rva2off(oft_rva), rva2off(fth_rva)
        while o is not None and f is not None:
            ord_or_name = struct.unpack_from('<Q', blob, o)[0]
            if ord_or_name == 0:
                break
            if ord_or_name & 0x8000000000000000:
                fname = f'ORD:{ord_or_name & 0xFFFF}'
            else:
                no2 = rva2off(ord_or_name)
                if no2 is not None and no2 + 4 <= len(blob):
                    end = blob.find(b'\0', no2 + 2)  # +2 跳过 hint
                    fname = blob[no2 + 2:end].decode('ascii', 'ignore')
                else:
                    fname = f'?rva{ord_or_name:x}'
            entries.append((fname, fth_rva + (f - rva2off(fth_rva))))
            o += 8
            f += 8
        yield dll_name, entries
        off += 20


def pe_exports(blob, pattern=''):
    """Yield (name, rva) for exports matching pattern."""
    mz = blob.find(b'MZ')
    if mz < 0:
        return
    e_lfanew = struct.unpack_from('<I', blob, mz + 0x3C)[0] + mz
    magic = struct.unpack_from('<H', blob, e_lfanew + 24)[0]
    dd_off = e_lfanew + 24 + (112 if magic == 0x20B else 96)
    exp_rva, exp_size = struct.unpack_from('<II', blob, dd_off + 0 * 8)
    if not exp_rva:
        return
    base = mz + exp_rva  # 导出目录在 blob[mz + rva]
    nfunc, nnames = struct.unpack_from('<II', blob, base + 20)
    arr_fn = struct.unpack_from('<I', blob, base + 28)[0]
    arr_nm = struct.unpack_from('<I', blob, base + 32)[0]
    arr_ord = struct.unpack_from('<I', blob, base + 36)[0]
    for i in range(nnames):
        name_rva = struct.unpack_from('<I', blob, mz + arr_nm + i * 4)[0]
        off = mz + name_rva
        end = blob.find(b'\0', off)
        name = blob[off:end].decode('ascii', 'ignore')
        if pattern in name:
            ord_ = struct.unpack_from('<H', blob, mz + arr_ord + i * 2)[0]
            fn_rva = struct.unpack_from('<I', blob, mz + arr_fn + ord_ * 4)[0]
            yield name, fn_rva


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('module')
    ap.add_argument('--dump', default=None)
    ap.add_argument('--base', default='0x0')
    ap.add_argument('--pattern', default='XPF_ParamStreamWrite')
    args = ap.parse_args()
    blob = open(args.module, 'rb').read()
    base = int(args.base, 16)
    import sys
    sys.path.insert(0, 'scripts/research')
    if args.dump:
        import dump_disasm as dd
        dbuf = open(args.dump, 'rb').read()
        streams = dd.parse_streams(dbuf)
        mem = dd.read_memory64(dbuf, streams)

    pat = args.pattern
    total = 0
    for dll, entries in pe_imports(blob):
        hit = [(n, iat) for n, iat in entries if pat in n]
        if hit:
            print(f'== {dll} ({len(hit)} hits) ==')
            for name, iat_rva in hit:
                total += 1
                print(f'  {name:<40} IAT rva={iat_rva:#x}')
                if args.dump:
                    va = base + iat_rva
                    code, resolved = dd.extract_bytes(dbuf, mem, va, 8)
                    if code:
                        impl = struct.unpack('<Q', code)[0]
                        print(f'    -> runtime impl VA: {impl:#x}')
    print(f'[done] {total} matching import(s)')


if __name__ == '__main__':
    main()