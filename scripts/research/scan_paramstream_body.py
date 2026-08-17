#!/usr/bin/env python3
"""scan_paramstream_body.py - find real ParamStream-serialized buffers in a
Xunlei DownloadSDKServer minidump, using the TLV layout decoded by the
cloud RE team:

    WriteUInt8   [0x08][1]      WriteInt8    [0x04][1]
    WriteUInt16  [0x09][2 LE]   WriteUInt32  [0x0a][4 LE]
    WriteInt32   [0x06][4 LE]   WriteUInt64  [0x0b][8 LE]
    WritePointer [0x0d][8 LE]   WriteBuffer  [0x0f][8B len][data]

A run of >= MIN_ITEMS self-consistent TLV items is a candidate serialized
body. Local-only; redacts credential-ish context (same policy as
scan_minidump.py).

Usage:
    python scan_paramstream_body.py <dump.dmp> [--min-items 4] [--max-hits 20]
"""
import argparse
import mmap
import os
import re
import sys

TAG_SIZES = {
    0x04: 1,  # Int8
    0x06: 4,  # Int32
    0x08: 1,  # UInt8
    0x09: 2,  # UInt16
    0x0a: 4,  # UInt32
    0x0b: 8,  # UInt64
    0x0d: 8,  # Pointer
    0x0f: 8,  # Buffer: [u64 len][data] - handled specially
}

SENSITIVE = re.compile(
    rb'(token|cookie|session|device_id|deviceid|passwd|password|pwd|auth|secret|'
    rb'account|user(name)?|sid)\s*[=:]\s*["\']?[A-Za-z0-9_./+=\-]{12,}',
    re.IGNORECASE,
)


def redact(b: bytes) -> str:
    out = SENSITIVE.sub(lambda m: m.group(0).split(b'=')[0] + b'=[REDACTED]', b)
    return ''.join(chr(c) if 32 <= c < 127 else '.' for c in out)


def item_len(data: bytes, off: int) -> int | None:
    """TLV item total length or None if invalid at off."""
    if off >= len(data):
        return None
    tag = data[off]
    n = TAG_SIZES.get(tag)
    if n is None:
        return None
    if tag == 0x0f:
        if off + 9 > len(data):
            return None
        ln = int.from_bytes(data[off + 1:off + 9], 'little')
        if ln > 16 * 1024 * 1024:  # unreasonable buffer len
            return None
        total = 9 + ln
        if off + total > len(data):
            return None
        # printable check: buffer should be at least partially decodable
        payload = data[off + 9:off + 9 + min(ln, 64)]
        printable = sum(1 for b in payload if 32 <= b < 127 or b == 0)
        if printable < max(1, len(payload) // 2):
            return None
        return total
    total = 1 + n
    return total if off + total <= len(data) else None


def scan(path: str, min_items: int, max_hits: int):
    size = os.path.getsize(path)
    found = 0
    with open(path, 'rb') as f:
        data = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        try:
            off = 0
            while off < size and found < max_hits:
                # Find first plausible tag, then try to extend a run
                run = []
                cur = off
                total = 0
                while True:
                    il = item_len(data, cur)
                    if il is None:
                        break
                    run.append((cur, data[cur], il))
                    total += il
                    cur += il
                    if len(run) >= 64 or total > 1 << 20:
                        break
                if len(run) >= min_items:
                    found += 1
                    start, end = run[0][0], cur
                    print(f'=== candidate #{found} @ {start:#x} (+{total} bytes, '
                          f'{len(run)} items) ===')
                    tags = ','.join(f'{t:#04x}' for _, t, _ in run[:12])
                    print(f'  tags: {tags}{"..." if len(run) > 12 else ""}')
                    ctx = bytes(data[max(0, start - 96):min(size, end + 96)])
                    print(f'  ctx:  {redact(ctx)}')
                    off = end  # advance past the run
                else:
                    off += 1
            print(f'\n[done] {found} candidate(s) (cap {max_hits})')
        finally:
            data.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('dump')
    ap.add_argument('--min-items', type=int, default=4)
    ap.add_argument('--max-hits', type=int, default=20)
    args = ap.parse_args()
    if not os.path.exists(args.dump):
        sys.exit(f'not found: {args.dump}')
    print(f'[i] scanning {args.dump} ({os.path.getsize(args.dump) / 1e6:.0f} MB) '
          f'for ParamStream TLV runs >= {args.min_items} items')
    scan(args.dump, args.min_items, args.max_hits)


if __name__ == '__main__':
    main()