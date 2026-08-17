#!/usr/bin/env python3
"""extract_http_body.py - pull real PHub/SHub HTTP request/response bodies out
of a Xunlei minidump by anchors (Content-Length + template), print hex.

Anchors found in the 23:44 window dump:
  - response: 'HTTP/1.1 200 OK' + 'Content-Length: 36'  (PHub/SHub response)
  - request:  'POST / HTTP/1.1' + 'Host: sr-shub.sandai.net' + 'Content-Length: 368'
Usage: python extract_http_body.py <dump.dmp> [--offsets 0x399ac7,0x3ab3ea1]
"""
import argparse
import mmap
import os
import re
import sys


def hexdump(b: bytes, width=16) -> str:
    lines = []
    for off in range(0, len(b), width):
        chunk = b[off:off + width]
        hexs = ' '.join(f'{c:02x}' for c in chunk)
        asc = ''.join(chr(c) if 32 <= c < 127 else '.' for c in chunk)
        lines.append(f'  {off:04x}: {hexs:<{width*3}} {asc}')
    return '\n'.join(lines)


def find_body(data, hdr: bytes, host_hint: bytes | None, label: str, out_dir: str | None):
    hits = []
    start = 0
    while True:
        i = data.find(hdr, start)
        if i < 0:
            break
        # 限 1KB 内找 Content-Length
        seg = bytes(data[i:i + 1024])
        m = re.search(rb'Content-Length:\s*(\d+)', seg, re.I)
        if not m:
            start = i + 1
            continue
        clen = int(m.group(1))
        # body 起始 = header 块结束后（\r\n\r\n 之后）
        body_off = i + seg.find(b'\r\n\r\n') + 4
        if body_off + clen > len(data) or clen <= 0 or clen > 1 << 20:
            start = i + 1
            continue
        body = bytes(data[body_off:body_off + clen])
        # host 提示过滤（可选）
        if host_hint and host_hint not in seg:
            start = i + 1
            continue
        print(f'=== {label} @ {i:#x} Content-Length={clen} ===')
        print(hexdump(body[: min(clen, 96)]))
        print(f'  (body {clen}B; first 8: {body[:8].hex()})')
        if out_dir:
            safe = re.sub(r'[^A-Za-z0-9._-]', '_', label).replace(' ', '_')
            fn = os.path.join(out_dir, f'{safe}_{i:#x}.bin')
            with open(fn, 'wb') as bf:
                bf.write(body)
            print(f'  (full body saved -> {fn})')
        print()
        hits.append((i, body))
        start = i + 1
    return hits


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('dump')
    ap.add_argument('--out-dir', default=None, help='save full bodies here (local only)')
    args = ap.parse_args()
    if not os.path.exists(args.dump):
        sys.exit(f'not found: {args.dump}')
    out_dir = args.out_dir
    if out_dir:
        os.makedirs(out_dir, exist_ok=True)
    with open(args.dump, 'rb') as f:
        data = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        try:
            print(f'[i] {args.dump} ({len(data) / 1e6:.0f} MB)\n')
            # 1) 200 OK 响应 (36B)
            find_body(
                data,
                b'HTTP/1.1 200 OK',
                None,
                'RESP_200_OK',
                out_dir,
            )
            # 2) POST 请求 (368B, sr-shub)
            find_body(
                data,
                b'POST / HTTP/1.1',
                b'sr-shub.sandai.net',
                'REQUEST_POST_sr-shub',
                out_dir,
            )
            # 3) POST 请求 (pr-phub 若存在)
            find_body(
                data,
                b'POST / HTTP/1.1',
                b'pr-phub.sandai.net',
                'REQUEST_POST_pr-phub',
                out_dir,
            )
        finally:
            data.close()


if __name__ == '__main__':
    main()