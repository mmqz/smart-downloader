#!/usr/bin/env python3
"""analyze_body.py - inspect a captured HTTP body: len, hexdump, printable runs,
entropy (help decide plaintext vs encrypted vs serialized)."""
import re
import sys


def analyze(path: str, head=256):
    data = open(path, 'rb').read()
    print(f'== {path} ({len(data)}B) ==')
    print(f'first 16: {data[:16].hex()}')
    print(f'-- head {min(head, len(data))}B --')
    for off in range(0, min(head, len(data)), 16):
        chunk = data[off:off + 16]
        hexs = ' '.join(f'{c:02x}' for c in chunk)
        asc = ''.join(chr(c) if 32 <= c < 127 else '.' for c in chunk)
        print(f'{off:05x}: {hexs:<48} {asc}')

    # entropy estimate over whole body (per 256B block avg)
    if len(data) >= 256:
        import math
        ent = []
        for b in range(0, len(data) - 255, 256):
            blk = data[b:b + 256]
            freq = [0] * 256
            for c in blk:
                freq[c] += 1
            e = -sum((f / 256) * math.log2(f / 256) for f in freq if f)
            ent.append(e)
        avg = sum(ent) / len(ent)
        print(f'entropy/byte (avg of {len(ent)} blocks): {avg:.2f} bits '
              f'(<6 ~text/bencode, >7.5 ~encrypted/compressed)')

    print('-- printable runs (>=5) --')
    for m in re.finditer(rb'[\x20-\x7e]{5,}', data):
        print(f'  @{m.start():05x}: {m.group(0).decode("latin-1")[:120]}')
    print()


for p in sys.argv[1:]:
    analyze(p)