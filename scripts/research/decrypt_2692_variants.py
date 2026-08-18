#!/usr/bin/env python3
"""decrypt_2692_variants.py - brute-force plausible (cmd, seq) MD5-derived
keys against the captured 2692B response (fixed-key hypothesis)."""
import hashlib
import struct


def aes_ecb_dec(key, ct):
    try:
        from Crypto.Cipher import AES
        return AES.new(key, AES.MODE_ECB).decrypt(ct)
    except ImportError:
        from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
        c = Cipher(algorithms.AES(key), modes.ECB())
        d = c.decryptor()
        return d.update(ct) + d.finalize()


def score(pt: bytes) -> int:
    # 可读性打分：可打印字节数 / 常见结构关键字
    n = sum(1 for b in pt[:128] if 32 <= b < 127 or b == 0)
    kw = sum(1 for k in (b'peer', b'cid', b'infohash', b'ip', b'port',
                         b'complete', b'download', b'torrent', b'error', b'code')
             if k in pt[:1024])
    return n + kw * 10


def main():
    req = open('scripts/research/captures/bodies/REQUEST_POST_sr-shub_0x3ab3ea1.bin', 'rb').read()
    resp = open('scripts/research/captures/replays/resp_0.bin', 'rb').read()
    body = resp[4:]  # 4B 长度头
    print(f'request first 16: {req[:16].hex()}')
    print(f'response body: {len(body)}B, aligned={len(body) % 16 == 0}')

    # 候选 cmd/seq（从请求头推断）
    cands = []
    hdr = req[:16]
    cands.append(('cmd[0:4]LE seq[5:9]LE', struct.unpack_from('<I', hdr, 0)[0],
                  struct.unpack_from('<I', hdr, 5)[0]))
    cands.append(('cmd[0:4]LE seq[4:8]LE', struct.unpack_from('<I', hdr, 0)[0],
                  struct.unpack_from('<I', hdr, 4)[0]))
    cands.append(('cmd[0:4]LE seq[5:9]BE', struct.unpack_from('<I', hdr, 0)[0],
                  struct.unpack_from('>I', hdr, 5)[0]))
    for full_seq in (39, 0x27, 0x80000027, 0x27800000):
        cands.append((f'cmd=1 seq={full_seq:#x}', 1, full_seq))
        cands.append((f'cmd=0x26035888 seq={full_seq:#x}', 0x26035888, full_seq))

    best = []
    # 也直接试原始 9B/10B 前缀 MD5
    for name, ct in [
        ('MD5(req[0:9])', req[0:9]),
        ('MD5(req[0:13])', req[0:13]),
    ]:
        cands.append((name, None, None))

    for label, cmd, seq in cands:
        if cmd is None:
            key = hashlib.md5(ct).digest()
        else:
            key = hashlib.md5(struct.pack('<I', seq) + struct.pack('<I', cmd)).digest()
        pt = aes_ecb_dec(key, body)
        s = score(pt)
        asc = ''.join(chr(c) if 32 <= c < 127 else '.' for c in pt[:64])
        best.append((s, label, pt, key))
        if s > 40:
            print(f'[{label}] key={key.hex()} score={s}')
            print(f'   pt: {asc}')
            print(f'   hex: {pt[:32].hex()}')

    best.sort(key=lambda x: -x[0])
    print('\n--- top candidates ---')
    for s, label, pt, key in best[:4]:
        print(f'[{label}] score={s} key={key.hex()} first16={pt[:16].hex()}')


if __name__ == '__main__':
    main()