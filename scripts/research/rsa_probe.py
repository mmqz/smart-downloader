#!/usr/bin/env python3
"""rsa_probe.py - parse RSA PEMs (stdlib-only BER for the common key layouts),
check pub/priv pairing (same modulus), and scan a dump for PKCS#1 v1.5
RSA ciphertexts (256B, starts 00 02 ...) that could be 'ekey' (RSA-encrypted
AES key). Local-only, no crypto deps.

Usage:
    python rsa_probe.py keys/pub_1.pem keys/priv.pem [--dump <x.dmp>]
"""
import argparse
import base64
import os
import re
import sys


def b64_payload(pem: str) -> bytes:
    m = re.search(r'-----BEGIN [A-Z ]*KEY-----\s*(.*?)\s*-----END', pem, re.S)
    if not m:
        raise ValueError('not a PEM')
    return base64.b64decode(''.join(m.group(1).split()))


def read_tlv(data: bytes, off: int):
    """Parse one BER TLV at off; return (tag_class_number, length, value_start, value_end)."""
    if off >= len(data):
        raise ValueError('truncated')
    tag = data[off]
    off += 1
    ln = data[off]
    off += 1
    if ln & 0x80:
        n = ln & 0x7F
        ln = int.from_bytes(data[off:off + n], 'big')
        off += n
    return tag, ln, off, off + ln


def parse_rsa_public_bits(pem: str):
    """SubjectPublicKeyInfo -> (N, e). Locate inner RSAPublicKey SEQUENCE."""
    der = b64_payload(pem)
    t, ln, p, end = read_tlv(der, 0)  # outer SEQ
    i = der.find(b'\x30\x82', p, end)  # inner RSA key SEQ (second 30 82)
    if i < 0:
        raise ValueError('SPKI layout unexpected: no inner SEQ')
    kln = int.from_bytes(der[i + 2:i + 4], 'big')
    t, vl, vs, ve = read_tlv(der, i + 4)  # INTEGER N
    if t != 0x02:
        raise ValueError('SPKI layout unexpected: N not INTEGER')
    N = int.from_bytes(der[vs:ve], 'big')
    t2, vl2, vs2, ve2 = read_tlv(der, ve)  # INTEGER e
    if t2 != 0x02:
        raise ValueError('SPKI layout unexpected: e not INTEGER')
    e = int.from_bytes(der[vs2:ve2], 'big')
    return N, e


def parse_rsa_private(pem: str):
    """PKCS#1 RSAPrivateKey -> (N, e, d)."""
    der = b64_payload(pem)
    t, ln, p, end = read_tlv(der, 0)  # SEQ
    vals = []
    while p < end:
        tag, vl, vs, ve = read_tlv(der, p)
        if tag == 0x02:  # INTEGER
            vals.append(int.from_bytes(der[vs:ve], 'big'))
        p = ve
    if len(vals) < 3:
        raise ValueError(f'fewer than 3 integers: {len(vals)}')
    return vals[1], vals[2], vals[3]  # version, N, e, d -> return N, e, d


def find_ekey_candidates(dump: bytes, modulus_bits: int = 256):
    """PKCS#1 v1.5 ciphertext: 00 02 || >=8 nonzero pad bytes || 00 . . . ."""
    k = modulus_bits  # bytes
    hits = []
    start = 0
    while True:
        i = dump.find(b'\x00\x02', start)
        if i < 0:
            break
        if i + k > len(dump):
            break
        block = dump[i:i + k]
        # pad: bytes 2..j nonzero then a 0x00
        j = 2
        while j < k and block[j] != 0:
            j += 1
        if j >= 10 and j < k:  # valid PKCS#1 v1.5 padding (EK >= 8 nonzero)
            # remainder after separator: candidate key material (16/32B typical)
            hits.append(i)
            start = i + 1
        else:
            start = i + 1
        if len(hits) >= 10:
            break
    return hits


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('pub')
    ap.add_argument('priv')
    ap.add_argument('--dump', default=None)
    args = ap.parse_args()

    Np, e = parse_rsa_public_bits(open(args.pub, encoding='utf-8', errors='ignore').read())
    print(f'pub : N (hex last 8) ...{Np.to_bytes(256, "big")[-8:].hex()}  e={e}')
    Nq, eq, d = parse_rsa_private(open(args.priv, encoding='utf-8', errors='ignore').read())
    print(f'priv: N (hex last 8) ...{Nq.to_bytes(256, "big")[-8:].hex()}  d bits={d.bit_length()}')
    print(f'PAIRING: {"YES - same modulus" if Np == Nq else "NO - different modulus"}')

    if args.dump and os.path.exists(args.dump):
        with open(args.dump, 'rb') as f:
            data = f.read()
        hits = find_ekey_candidates(data)
        print(f'\nekey candidates in dump (PKCS#1 00 02 256B): {len(hits)}')
        for h in hits[:8]:
            blk = data[h:h + 256]
            sep = blk.index(0) if 0 in blk[2:] else 0
            payload = blk[sep + 1:sep + 1 + 48] if sep else b''
            print(f'  @{h:#x}: pad={sep - 2:2d}B, post-sep first 16: {payload[:16].hex()}')
            if Np == Nq:
                # try decrypt as ekey
                try:
                    m = pow(int.from_bytes(blk, 'big'), d, Np)
                    mb = m.to_bytes(256, 'big')
                    print(f'    RSA-decrypt -> ...{mb[:40].hex()}')
                except Exception as ex:
                    print(f'    decrypt err {ex}')


if __name__ == '__main__':
    main()