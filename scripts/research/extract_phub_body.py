#!/usr/bin/env python3
"""extract_phub_body.py - pull the real PHub HTTP POST body out of a pktmon pcapng.

Pure standard library (no scapy/tshark needed). Pipeline:
    capture_phub.ps1  (admin) -> phub-<ts>.pcapng
    python extract_phub_body.py phub-<ts>.pcapng [-o body.bin] [--hex out.hex]

Steps: parse pcapng blocks (SHB/IDB/EPB) -> reassemble TCP streams ->
find the stream whose client side sends "POST / HTTP/1.1" -> read body
by Content-Length -> dump hex + ascii preview.

Evidence level: this is a REAL packet sample (A-level) cross-checked against
the ParamStream serialization notes in docs/research/xunlei/p2p_research_complete.md.
"""
import argparse
import struct
import sys
from collections import defaultdict

ETH_HDR = 14
IP_HDR_MIN = 20
TCP_HDR_MIN = 20

def parse_pcapng(path):
    """Yield (linktype, caplen, data) for each captured packet."""
    with open(path, 'rb') as f:
        buf = f.read()
    off = 0
    linktype = None
    while off + 12 <= len(buf):
        btype, blen = struct.unpack_from('<II', buf, off)
        if blen < 12 or off + blen > len(buf):
            break
        body = buf[off + 8: off + blen - 4]  # drop trailing block_total_length
        if btype == 0x0A0D0D0A:  # SHB
            linktype = None
        elif btype == 0x00000001:  # IDB
            if len(body) >= 8:
                linktype = struct.unpack_from('<H', body, 0)[0]
        elif btype == 0x00000006:  # EPB: iface(4) ts_hi(4) ts_lo(4) caplen(4) origlen(4) data...
            if len(body) >= 20:
                caplen = struct.unpack_from('<I', body, 12)[0]
                data = body[20:20 + caplen]
                yield linktype, caplen, data
        elif btype == 0x00000003:  # SPB: origlen(4) data...
            if len(body) >= 4:
                caplen = struct.unpack_from('<I', body, 0)[0]
                data = body[4:4 + caplen]
                yield linktype, caplen, data
        off += blen

def parse_ipv4_tcp(data, linktype):
    """Extract (src_ip, src_port, dst_ip, dst_port, seq, payload) or None."""
    if linktype == 1:  # Ethernet
        if len(data) < ETH_HDR + IP_HDR_MIN:
            return None
        ethertype = struct.unpack_from('>H', data, 12)[0]
        if ethertype != 0x0800:
            return None
        ip = data[ETH_HDR:]
    elif linktype == 101:  # raw IPv4
        ip = data
    else:
        return None
    if len(ip) < IP_HDR_MIN or (ip[0] >> 4) != 4:
        return None
    ihl = (ip[0] & 0x0F) * 4
    proto = ip[9]
    if proto != 6 or len(ip) < ihl + TCP_HDR_MIN:
        return None
    src = '.'.join(str(b) for b in ip[12:16])
    dst = '.'.join(str(b) for b in ip[16:20])
    tcp = ip[ihl:]
    sport, dport = struct.unpack_from('>HH', tcp, 0)
    seq = struct.unpack_from('>I', tcp, 4)[0]
    doff = (tcp[12] >> 4) * 4
    payload = tcp[doff:]
    return (src, sport, dst, dport, seq, payload)

def reassemble(path):
    """Return dict stream_key -> [(seq, payload)] in capture order."""
    streams = defaultdict(list)
    for linktype, _caplen, data in parse_pcapng(path):
        r = parse_ipv4_tcp(data, linktype)
        if not r:
            continue
        src, sport, dst, dport, seq, payload = r
        key = (src, sport, dst, dport)
        streams[key].append((seq, payload))
    return streams

def extract_post_body(streams, target_ip='140.206.220.33'):
    """Find the client->server HTTP stream with a POST / request; return its body."""
    for key, chunks in streams.items():
        src, sport, dst, dport = key
        if dport != 80:
            continue
        # order by seq
        chunks.sort()
        stream = b''
        for _seq, payload in chunks:
            stream += payload
        if b'POST / HTTP/1.1' not in stream:
            continue
        header_end = stream.find(b'\r\n\r\n')
        if header_end < 0:
            print(f'[!] stream {src}:{sport}->{dst}:{dport} has POST but no header terminator')
            continue
        headers = stream[:header_end].decode('latin-1', errors='replace')
        body = stream[header_end + 4:]
        clen = 0
        for line in headers.split('\r\n'):
            if line.lower().startswith('content-length:'):
                clen = int(line.split(':', 1)[1].strip())
        print(f'[OK] POST stream {src}:{sport}->{dst}:{dport}, Content-Length={clen}, '
              f'got {len(body)} bytes after header')
        return headers, body[:clen]
    return None, None

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('pcapng')
    ap.add_argument('-o', '--out', default='phub_body.bin')
    ap.add_argument('--hex', default='phub_body.hex')
    args = ap.parse_args()

    streams = reassemble(args.pcapng)
    print(f'[i] {len(streams)} TCP streams')
    headers, body = extract_post_body(streams)
    if body is None:
        print('[FAIL] no PHub POST found (empty capture? wrong filter? Xunlei idle?)')
        sys.exit(1)

    with open(args.out, 'wb') as f:
        f.write(body)
    with open(args.hex, 'w') as f:
        for i in range(0, len(body), 16):
            chunk = body[i:i + 16]
            f.write(f'{i:08x}  ' + ' '.join(f'{b:02x}' for b in chunk).ljust(48)
                    + '  ' + ''.join(chr(b) if 32 <= b < 127 else '.' for b in chunk) + '\n')
    print(f'[i] body saved: {args.out} ({len(body)} bytes), hex: {args.hex}')
    print('[i] first 64 bytes:')
    print('    ' + ' '.join(f'{b:02x}' for b in body[:64]))

if __name__ == '__main__':
    main()
