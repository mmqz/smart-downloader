#!/usr/bin/env python3
"""dumptext.py - print printable runs from a blob/dump region (local analysis)."""
import re
import sys

def show(path, limit=900):
    data = open(path, 'rb').read()
    print(f'===== {path} ({len(data)}B) =====')
    runs = re.finditer(rb'[\x20-\x7e]{4,}', data)
    for i, m in enumerate(runs):
        if i >= limit:
            break
        s = m.group(0)
        print(f'  @{m.start():06x} {s.decode("latin-1")[:200]}')

for p in sys.argv[1:]:
    show(p)
    print()