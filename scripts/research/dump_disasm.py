#!/usr/bin/env python3
"""dump_disasm.py - locate a module in a minidump, extract its code bytes, and
disassemble an RVA range with capstone (x64). Used to RE PhubHttpPkgRequester's
SERIALIZE_FN (Http.dll RVA 0x192a0 per cloud analysis) against real samples.

Usage:
    python dump_disasm.py <dump.dmp> --module Http.dll --rva 0x192a0 --len 0x800
"""
import argparse
import struct
import sys

try:
    from capstone import Cs, CS_ARCH_X86, CS_MODE_64
except ImportError:
    sys.exit('capstone not installed (pip install capstone)')


def parse_streams(buf):
    magic = buf[0:4]
    if magic != b'MDMP':
        raise ValueError(f'not MDMP: {magic!r}')
    num = struct.unpack_from('<I', buf, 8)[0]
    dir_rva = struct.unpack_from('<I', buf, 12)[0]
    streams = {}
    for i in range(num):
        stype, size, rva = struct.unpack_from('<III', buf, dir_rva + i * 12)
        streams[stype] = (rva, size)
    return streams


def read_module_list(buf, streams):
    rva, size = streams.get(4)  # ModuleListStream
    if rva is None:
        return []
    count = struct.unpack_from('<I', buf, rva)[0]
    mods = []
    p = rva + 4
    for _ in range(count):
        base, img_size, _chk, _ts = struct.unpack_from('<QIII', buf, p)
        name_rva = struct.unpack_from('<I', buf, p + 20)[0]
        nlen = struct.unpack_from('<I', buf, name_rva)[0]
        name = buf[name_rva + 4:name_rva + 4 + nlen].decode('utf-16-le', 'ignore')
        mods.append((base, img_size, name))
        p += 108  # MINIDUMP_MODULE size
    return mods


def read_memory64(buf, streams):
    rva, size = streams.get(9)  # Memory64ListStream
    if rva is None:
        return None
    count, base_rva = struct.unpack_from('<QQ', buf, rva)
    ranges = []
    p = rva + 16
    for _ in range(count):
        start_va, data_size = struct.unpack_from('<QQ', buf, p)
        ranges.append((start_va, data_size))
        p += 16
    return base_rva, ranges


def extract_bytes(buf, mem, va, length):
    base_rva, ranges = mem
    for start_va, data_size in ranges:
        if start_va <= va < start_va + data_size:
            off = base_rva + (va - start_va)
            if off + length > len(buf):
                length = len(buf) - off
            return buf[off:off + length], va
    return None, va


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('dump')
    ap.add_argument('--module', default='Http.dll')
    ap.add_argument('--rva', default='0x192a0')
    ap.add_argument('--len', default='0x400')
    ap.add_argument('--export', default=None,
                    help='export ENTIRE module image to file (skip disasm)')
    args = ap.parse_args()

    with open(args.dump, 'rb') as f:
        buf = f.read()
    streams = parse_streams(buf)
    mods = read_module_list(buf, streams)
    print(f'[i] modules: {len(mods)}')
    for base, size, name in mods:
        if args.module.lower() in name.lower():
            print(f'  target: {name} base={base:#x} size={size:#x}')
            if args.export:
                mem = read_memory64(buf, streams)
                # detours 修补过的 PE 节可能超出 SizeOfImage，扩大导出覆盖全部节
                blob, _ = extract_bytes(buf, mem, base, max(size, 0x100000))
                if blob is None:
                    print('  [ERR] module range not mapped')
                    return
                import os
                d = os.path.dirname(os.path.abspath(args.export))
                os.makedirs(d, exist_ok=True)
                with open(args.export, 'wb') as g:
                    g.write(blob)
                print(f'  exported {len(blob)}B -> {args.export}')
                return
            va = base + int(args.rva, 16)
            length = int(args.len, 16)
            code, resolved = extract_bytes(buf, read_memory64(buf, streams), va, length)
            if code is None:
                print('  [ERR] RVA not mapped in memory64 (module code may be truncated)')
                return
            print(f'  extracted {len(code)}B @ {resolved:#x}')
            md = Cs(CS_ARCH_X86, CS_MODE_64)
            md.skipdata = True  # 跳过无效字节，遇 VEX/数据夹层继续
            for insn in md.disasm(code, resolved):
                print(f'    {insn.address:#x}: {insn.mnemonic:<8} {insn.op_str}')
            return
    print(f'[ERR] module not found: {args.module}')


if __name__ == '__main__':
    main()