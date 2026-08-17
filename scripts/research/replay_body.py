#!/usr/bin/env python3
"""replay_body.py - replay a captured encrypted body as an HTTP POST to its
original host. Tests whether Xunlei's hub accepts replay of an already-valid
request (no key needed if the server does not do strong anti-replay).

Security: only sends the captured ciphertext from the user's own machine to
the user's own account's hub; response is printed locally.
"""
import argparse
import http.client
import socket
import sys


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('body', help='bin file with the encrypted body')
    ap.add_argument('--host', default='sr-shub.sandai.net')
    ap.add_argument('--port', type=int, default=80)
    ap.add_argument('--path', default='/')
    args = ap.parse_args()

    body = open(args.body, 'rb').read()
    print(f'[i] replay {len(body)}B to {args.host}:{args.port}{args.path}')

    # resolve
    try:
        ips = socket.getaddrinfo(args.host, args.port, proto=socket.IPPROTO_TCP)
        print(f'[i] resolved: {[x[4][0] for x in ips[:3]]}')
    except socket.gaierror as ex:
        print(f'[ERR] resolve failed: {ex}')
        return

    conn = http.client.HTTPConnection(args.host, args.port, timeout=15)
    try:
        conn.request('POST', args.path, body=body, headers={
            'Content-type': 'application/octet-stream',
            'Content-Length': str(len(body)),
            'Connection': 'Close',
            'Host': args.host,
        })
        resp = conn.getresponse()
        data = resp.read()
        print(f'[RESP] {resp.status} {resp.reason}')
        print(f'[HEAD] {dict((k, v) for k, v in resp.getheaders())}')
        print(f'[BODY] {len(data)}B first 64 hex: {data[:64].hex()}')
        asc = ''.join(chr(c) if 32 <= c < 127 else '.' for c in data[:128])
        print(f'[BODY] ascii: {asc}')
    except Exception as ex:
        print(f'[ERR] {type(ex).__name__}: {ex}')
    finally:
        conn.close()


if __name__ == '__main__':
    main()