#!/usr/bin/env python3
"""scan_minidump.py - local-only scanner for Xunlei DownloadSDKServer minidumps.

Finds PHub/QAClient/ParamStream anchors in the raw dump bytes and prints
REDACTED context (never prints credentials/tokens). Dump file stays on this
machine; only the redacted report leaves it.

Usage:
    python scan_minidump.py <dump.dmp> [--blob-out dir] [--max-hits N]
"""
import argparse
import mmap
import os
import re
import sys

ANCHORS = [
    (b'pr-phub.sandai.net', 'PHub host string'),
    (b'QAClientPackage', 'QAClient package type name'),
    (b'QAClient', 'QAClient class/name'),
    (b'XDL_QAClientPackageParser', 'QAClient parser'),
    (b'PhubHttpPkgRequester', 'PHub http requester class'),
    (b'ParamStream', 'ParamStream serializer'),
    (b'XPF_ParamStream', 'XPF ParamStream API'),
    (b'CreateParamStream', 'CreateParamStream symbol'),
    (b'Content-Type: application/octet-stream', 'HTTP template'),
    (b'POST / HTTP/1.1', 'HTTP POST template'),
    (b'HubClient', 'HubClient connection type'),
    (b'UdpConnection.HubClient', 'Hub connection type register'),
    (b'sandai.net', 'sandai host suffix'),
]

# Anything matching these is treated as credential-ish and redacted in output.
SENSITIVE = re.compile(
    rb'(token|cookie|session|device_id|deviceid|passwd|password|pwd|auth|secret|'
    rb'account|user(name)?|sid)\s*[=:]\s*["\']?[A-Za-z0-9_./+=\-]{12,}',
    re.IGNORECASE,
)

CTX = 160  # context bytes printed around each hit


def redact(b: bytes) -> str:
    out = SENSITIVE.sub(lambda m: m.group(0).split(b'=')[0] + b'=[REDACTED]', b)
    printable = ''.join(chr(c) if 32 <= c < 127 else '.' for c in out)
    return printable


def scan(path: str, blob_dir: str | None, max_hits: int):
    size = os.path.getsize(path)
    with open(path, 'rb') as f:
        if f.read(4) != b'MDMP':
            print(f'[WARN] not an MDMP minidump? magic mismatch (size={size})')
        data = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        try:
            for anchor, label in ANCHORS:
                hits = []
                start = 0
                while True:
                    i = data.find(anchor, start)
                    if i < 0:
                        break
                    hits.append(i)
                    start = i + 1
                print(f'=== {label}: {len(hits)} hit(s) ===')
                for j, off in enumerate(hits[:max_hits]):
                    lo = max(0, off - CTX)
                    hi = min(size, off + len(anchor) + CTX)
                    ctx = bytes(data[lo:hi])
                    print(f'  @ {off:#x} : ...{redact(ctx)}...')
                    if blob_dir:
                        safe = re.sub(rb'[^A-Za-z0-9._-]', b'_', label.encode()).decode()
                        blob = os.path.join(blob_dir, f'{off:#x}_{safe}.blob')
                        os.makedirs(blob_dir, exist_ok=True)
                        with open(blob, 'wb') as bf:
                            bf.write(bytes(data[max(0, off - 8192):min(size, off + 8192)]))
                        print(f'    (context saved -> {blob})')
                print()
        finally:
            data.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('dump')
    ap.add_argument('--blob-out', default=None, help='save 16KB contexts around hits here (local only)')
    ap.add_argument('--max-hits', type=int, default=6)
    args = ap.parse_args()
    if not os.path.exists(args.dump):
        sys.exit(f'not found: {args.dump}')
    print(f'[i] scanning {args.dump} ({os.path.getsize(args.dump) / 1e6:.0f} MB)')
    scan(args.dump, args.blob_out, args.max_hits)


if __name__ == '__main__':
    main()