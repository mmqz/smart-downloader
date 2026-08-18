#!/usr/bin/env python3
"""forge_phub_test.py - forge a PHubQueryRes request (cloud v2 algorithm),
send to the real pr-phub server, and try to decrypt the response.

Verifies: AES key = MD5(seq_no||cmd_id=1), 13B header + AES-ECB payload,
server reachable on :80 with DNS restored.
"""
import hashlib
import os
import socket
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'cloud_delivery'))
from peer_accelerator import (
    build_pHub_query_request, send_pHub_request, parse_pHub_header,
    derive_aes_key, aes_ecb_decrypt, pkcs7_unpad,
)

HOST = 'pr-phub.sandai.net'
PORT = 80


def try_decrypt(request, response):
    print(f'  response: {len(response)}B')
    # 响应可能带 13B 头（cmd=32）
    hdr = parse_pHub_header(response)
    if hdr:
        print(f'  resp header: cmd={hdr["cmd_id"]} flag={hdr["flag"]:#x} '
              f'seq={hdr["seq_no"]} enc_len={hdr["enc_len"]}')
        body = response[13:13 + hdr['enc_len']]
        # 响应解密用请求的 key（MD5(请求seq || 请求cmd=1)）
        req_hdr = parse_pHub_header(request)
        for label, key in [
            ('key=MD5(req_seq||req_cmd)', derive_aes_key(req_hdr['cmd_id'], req_hdr['seq_no'])),
            ('key=MD5(resp_seq||resp_cmd)', derive_aes_key(hdr['cmd_id'], hdr['seq_no'])),
        ]:
            pt = aes_ecb_decrypt(key, body[: len(body) - len(body) % 16])
            pt = pkcs7_unpad(pt)
            asc = ''.join(chr(c) if 32 <= c < 127 else '.' for c in pt[:128])
            if any(c in pt[:64] for c in (b'peer', b'cid', b'infohash', b'error', b'ip')) or \
                    sum(1 for c in pt[:64] if 32 <= c < 127) > 40:
                print(f'  [{label}] DECRYPTED: {asc}')
            else:
                print(f'  [{label}] garbage: {pt[:16].hex()}')
    else:
        body = response[4:] if len(response) > 4 and response[:4] == b'\x80\x0a\x00\x00' else response
        print(f'  no 13B header, first 4: {response[:4].hex()}, 16-aligned body: {len(body) % 16 == 0}')


def main():
    print(f'[i] DNS 现状: {HOST} ->')
    try:
        ips = set(x[4][0] for x in socket.getaddrinfo(HOST, PORT, proto=socket.IPPROTO_TCP))
        print(f'    {list(ips)[:3]}')
    except socket.gaierror as e:
        print(f'    resolve failed: {e}')
        return

    # forge 一个查询（随机 cid/gcid 测试协议握手；真实 cid 后续从迅雷进程捞）
    cid = os.urandom(16)
    gcid = os.urandom(16)
    seq = 0x0A000001
    req = build_pHub_query_request(cid, gcid, seq_no=seq)
    print(f'[i] forged {len(req)}B (cmd=1 flag=0xb seq={seq:#x})')
    print(f'[i] POST {HOST}:{PORT}/ ...')
    try:
        resp = send_pHub_request(req, HOST, PORT, timeout=12)
    except Exception as e:
        print(f'[ERR] send failed: {type(e).__name__}: {e}')
        return
    try_decrypt(req, resp)


if __name__ == '__main__':
    main()