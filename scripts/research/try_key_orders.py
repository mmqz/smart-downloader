#!/usr/bin/env python3
"""try_key_orders.py - brute-force the PHub AES key byte order against the
real server oracle ("decrypt request failed" vs any other response)."""
import hashlib
import os
import socket
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'cloud_delivery'))
from peer_accelerator import send_pHub_request, pkcs7_pad, aes_ecb_encrypt
import peer_accelerator as pa

SEQ = 0x0A000001
HOST = 'pr-phub.sandai.net'


def build(key):
    cid, gcid = os.urandom(16), os.urandom(16)
    plain = cid + gcid + struct.pack('<Q', 0) + b'-XL0019-' + bytes(12) \
        + struct.pack('<H', 0) + b'\x00\x00'
    enc = aes_ecb_encrypt(key, pkcs7_pad(plain))
    return pa.build_pHub_header(1, 0xB, SEQ, len(enc)) + enc


def try_order(label, pack):
    key = hashlib.md5(pack(1, SEQ)).digest()
    req = build(key)
    try:
        resp = send_pHub_request(req, HOST, 80, timeout=12)
    except Exception as e:
        print(f'[{label}] ERR {type(e).__name__}: {e}')
        return
    asc = ''.join(chr(c) if 32 <= c < 127 else '.' for c in resp[:64])
    print(f'[{label}] ({len(resp)}B) {asc}')
    print(f'        hex: {resp[:16].hex()}')


if __name__ == '__main__':
    print(f'[i] Pinging {HOST}:80 oracle with 4 key orders (seq={SEQ:#x})\n')
    try_order('MD5(seqLE||cmdLE)', lambda c, s: struct.pack('<I', s) + struct.pack('<I', c))
    try_order('MD5(cmdLE||seqLE)', lambda c, s: struct.pack('<I', c) + struct.pack('<I', s))
    try_order('MD5(seqLE||cmdLE)+magic0', lambda c, s: struct.pack('<I', s) + struct.pack('<I', c) + b'\x00')
    # 无加密对照：直接发不带 AES 的包
    cid, gcid = os.urandom(16), os.urandom(16)
    plain = cid + gcid + struct.pack('<Q', 0) + b'-XL0019-' + bytes(12) \
        + struct.pack('<H', 0) + b'\x00\x00'
    req = pa.build_pHub_header(1, 0xB, SEQ, len(plain)) + plain
    try:
        resp = send_pHub_request(req, HOST, 80, timeout=12)
        asc = ''.join(chr(c) if 32 <= c < 127 else '.' for c in resp[:64])
        print(f'[plain-noAES] ({len(resp)}B) {asc}')
        print(f'        hex: {resp[:16].hex()}')
    except Exception as e:
        print(f'[plain-noAES] ERR {type(e).__name__}: {e}')