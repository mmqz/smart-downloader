"""
PoC: 迅雷 PHub HTTP 客户端 - 最小复现

基于反汇编已确认的事实:
1. PHub 走 HTTP POST / (实测 pr-phub.sandai.net 返回 "decrypt request failed")
2. body 用 AES-ECB 加密 (XPF_AES* 反汇编 + PAM 2012 论文双印证)
3. SHub 走 GET /querybt.fcg?infoid=<infohash_hex> (反汇编字符串确认)
4. PHub 包含字段: SkipLength, ProtocolLength, ParseLength, RealLength, peerid
5. HUB_PROTO 完整 enum 已知

未验证:
- AES key 怎么派生 (PAM 2012 说"密钥内嵌消息头", 但具体位置需反汇编)
- PHub 包头具体字段顺序

策略:
1. 先实现完整的 PHub 包构造框架 (用已知字段)
2. 用多种 AES key 派生策略尝试解密响应
3. 真实发请求到 pr-phub.sandai.net,看响应能否解密
"""
import struct
import hashlib
import socket
import ssl
import time
import urllib.request, urllib.error
import os
from pathlib import Path

# ============= 迅雷客户端身份 (alist 已开源) =============
CLIENT_ID = "Xp6vsxz_7IYVw2BB"
CLIENT_SECRET = "Xp6vsy4tN9toTVdMSpomVdXpRmES"
CLIENT_VERSION = "8.31.0.9726"
PACKAGE_NAME = "com.xunlei.downloadprovider"
APPID = "40"
APPKEY = "34a062aaa22f906fca4fefe9fb3a3021"
USER_AGENT = ("ANDROID-com.xunlei.downloadprovider/8.31.0.9726 netWorkType/5G "
              "appid/40 deviceName/Xiaomi_M2004j7ac deviceModel/M2004J7AC "
              "OSVersion/12 protocolVersion/301 platformVersion/10 sdkVersion/512000 "
              "Oauth2Client/0.9 (Linux 4_14_186-perf-gddfs8vbb238b) (JAVA 0)")

ALGORITHMS = [
    "9uJNVj/wLmdwKrJaVj/omlQ",
    "Oz64Lp0GigmChHMf/6TNfxx7O9PyopcczMsnf",
    "Eb+L7Ce+Ej48u",
    "jKY0",
    "ASr0zCl6v8W4aidjPK5KHd1Lq3t+vBFf41dqv5+fnOd",
    "wQlozdg6r1qxh0eRmt3QgNXOvSZO6q/GXK",
    "gmirk+ciAvIgA/cxUUCema47jr/YToixTT+Q6O",
    "5IiCoM9B1/788ntB",
    "P07JH0h6qoM6TSUAK2aL9T5s2QBVeY9JWvalf",
    "+oK0AN",
]

# ============= 已确认的 PHub 服务器 =============
# 这些沙箱可访问 (前面实测):
PHUB_HOST = "pr-phub.sandai.net"          # IP: 140.206.220.33 (上海电信)
SHUB_HOST = "hub5btmain.sandai.net"        # IP: 112.64.218.154 (上海电信)
DCDN_HOST = "dcdnhub-xcloud.sandai.net"    # IP: 140.206.225.182

# ============= AES 实现 (用 Python 自带 cryptography 或 pyaes) =============
def aes_ecb_encrypt(key: bytes, data: bytes) -> bytes:
    """AES-ECB 加密, 自动 PKCS7 padding"""
    try:
        from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
        from cryptography.hazmat.backends import default_backend
    except ImportError:
        # 用 pyaes 或自实现
        return _aes_ecb_pure_python(key, data, encrypt=True)
    # PKCS7 padding
    pad_len = 16 - (len(data) % 16)
    data = data + bytes([pad_len] * pad_len)
    cipher = Cipher(algorithms.AES(key), modes.ECB(), backend=default_backend())
    encryptor = cipher.encryptor()
    return encryptor.update(data) + encryptor.finalize()


def aes_ecb_decrypt(key: bytes, data: bytes) -> bytes:
    """AES-ECB 解密, 自动 PKCS7 unpad"""
    try:
        from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
        from cryptography.hazmat.backends import default_backend
    except ImportError:
        return _aes_ecb_pure_python(key, data, encrypt=False)
    cipher = Cipher(algorithms.AES(key), modes.ECB(), backend=default_backend())
    decryptor = cipher.decryptor()
    plain = decryptor.update(data) + decryptor.finalize()
    # PKCS7 unpad
    if plain and 1 <= plain[-1] <= 16:
        pad_len = plain[-1]
        if plain[-pad_len:] == bytes([pad_len] * pad_len):
            plain = plain[:-pad_len]
    return plain


def _aes_ecb_pure_python(key, data, encrypt):
    """纯 Python AES 实现 (慢, 仅用于测试)"""
    # 装一下 cryptography, 如果不可用就用这个
    raise NotImplementedError("需要 pip install cryptography 或 pyaes")


def md5_hex(s):
    return hashlib.md5(s.encode() if isinstance(s, str) else s).hexdigest()


def sha1_hex(s):
    return hashlib.sha1(s.encode() if isinstance(s, str) else s).hexdigest()


def get_captcha_sign(device_id):
    timestamp = str(int(time.time() * 1000))
    s = f"{CLIENT_ID}{CLIENT_VERSION}{PACKAGE_NAME}{device_id}{timestamp}"
    for algo in ALGORITHMS:
        s = md5_hex(s + algo)
    return timestamp, f"1.{s}"


def generate_device_sign(device_id):
    base = f"{device_id}{PACKAGE_NAME}{APPID}{APPKEY}"
    sha1 = sha1_hex(base)
    md5 = md5_hex(sha1)
    return f"div101.{device_id}{md5}"


def generate_device_id(seed):
    if len(seed) == 32:
        return seed
    return md5_hex(seed)


# ============= PHub 包构造 =============
# 已知 PHub 包头字段 (从反汇编 + 论文):
#   SkipLength / ProtocolLength / ParseLength / RealLength / peerid / userid
# 但具体字节布局未完全确认 (D 级)
# PAM 2012 论文: Thunder Packet = Header(未加密) + Body(加密)
# Header = 4B 命令字 + 变长 Connection 部分

# PHub 命令字 (从 PHUB__GATEWAY__COMMID__ 推断)
CMD_ID_PING_REQ          = 0  # PHUB__PING__COMMID__PING_REQ
CMD_ID_PING_RESP         = 1  # PHUB__PING__COMMID__PING_RESP
CMD_ID_LOGOUT            = 2  # PHUB__PING__COMMID__LOGOUT
CMD_ID_QUERY_RES_REQ     = 3  # PHUB__GATEWAY__COMMID__QUERY_RES_REQ
CMD_ID_QUERY_RES_RESP    = 4  # PHUB__GATEWAY__COMMID__QUERY_RES_RESP
CMD_ID_REPORT_RCS_REQ    = 5
CMD_ID_REPORT_RCS_RESP   = 6
CMD_ID_DELETE_RCS_REQ    = 7
CMD_ID_DELETE_RCS_RESP   = 8
CMD_ID_INVALID_PEER_REQ  = 9
CMD_ID_RES_NEED_REPORT   = 10

# 这些数值是推测的 — 真实值需要更深反汇编或抓包验证


def build_phub_query_res_request(infohash: bytes, device_id: str) -> bytes:
    """构造 CmdPHubQueryRes 请求包
    
    字段 (从反汇编推断, D 级):
      - cmd_id (4B): PHUB__GATEWAY__COMMID__QUERY_RES_REQ
      - peerid (20B): 标准 BT peerid 格式 -XL0019-...
      - userid (8B): 0 (匿名)
      - task_scene (4B): TSC_DL = 下载场景
      - task_mode (4B): TMD_UNSPECIFIED
      - info_hash (20B): BT infohash
      - protocol_flag (4B): PRF_DEFAULT
      - hub_type (4B): HT_PHUB
    """
    peerid = b"-XL0019-" + os.urandom(12)  # 标准 BT peerid
    
    # 暂用推测布局 - 需要真实抓包验证
    body = struct.pack("<I", CMD_ID_QUERY_RES_REQ)
    body += peerid                                    # 20 字节 peerid
    body += struct.pack("<Q", 0)                      # userid = 0 (匿名)
    body += struct.pack("<I", 5)                      # task_scene = TSC_DL
    body += struct.pack("<I", 0)                      # task_mode = TMD_UNSPECIFIED
    body += infohash                                  # 20 字节 BT infohash
    body += struct.pack("<I", 0)                      # protocol_flag = PRF_DEFAULT
    body += struct.pack("<I", 1)                      # hub_type = HT_PHUB
    
    return body


def try_aes_keys(device_id: str, response_body: bytes) -> list:
    """尝试多种 AES key 派生策略
    
    PAM 2012 说"密钥内嵌消息头",但没有具体说明
    候选策略:
      1. device_id 前 16 字节 (MD5 = 16 字节)
      2. APPKEY 前 16 字节
      3. captcha_sign 前 16 字节 (md5)
      4. device_sign 末 32 字节前 16 字节 (md5)
      5. peerid 前 16 字节
    """
    candidates = []
    
    # 1. device_id MD5 (16 字节)
    candidates.append(("device_id_md5", hashlib.md5(device_id.encode()).digest()))
    
    # 2. APPKEY 直接 (32 字节, 取前 16)
    candidates.append(("appkey_first16", APPKEY.encode()[:16]))
    
    # 3. APPKEY MD5 (16 字节)
    candidates.append(("appkey_md5", hashlib.md5(APPKEY.encode()).digest()))
    
    # 4. captcha_sign MD5
    ts, sign = get_captcha_sign(device_id)
    candidates.append(("captcha_sign_md5", hashlib.md5(sign.encode()).digest()))
    
    # 5. device_sign 后 32 字节 (md5 部分) 前 16
    ds = generate_device_sign(device_id)
    md5_part = ds[-32:]
    candidates.append(("device_sign_md5_first16", bytes.fromhex(md5_part[:32])[:16]))
    
    # 6. 固定盐
    candidates.append(("appkey_full_16", APPKEY[:16].encode()))
    
    results = []
    for name, key in candidates:
        if len(key) not in [16, 24, 32]:
            continue
        try:
            decrypted = aes_ecb_decrypt(key, response_body)
            # 检查解密结果是否"像"可读数据
            non_zero = sum(1 for b in decrypted if b != 0)
            printable = sum(1 for b in decrypted if 32 <= b < 127)
            results.append({
                "key_name": name,
                "key_hex": key.hex(),
                "decrypted_head_hex": decrypted[:32].hex(),
                "decrypted_head_ascii": decrypted[:32].decode('ascii', errors='replace'),
                "non_zero_bytes": non_zero,
                "printable_bytes": printable,
                "total_bytes": len(decrypted),
            })
        except Exception as e:
            results.append({"key_name": name, "error": str(e)})
    return results


def send_phub_request(host: str, port: int, body: bytes, use_https: bool = True) -> tuple:
    """发送 PHub HTTP 请求"""
    scheme = "https" if use_https else "http"
    url = f"{scheme}://{host}:{port}/"
    
    # PHub 用 application/octet-stream (实测确认)
    req = urllib.request.Request(
        url,
        data=body,
        headers={
            'User-Agent': 'curl/7.64',
            'Content-Type': 'application/octet-stream',
            'Host': host,
        },
        method='POST',
    )
    
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    
    try:
        r = urllib.request.urlopen(req, timeout=10, context=ctx if use_https else None)
        return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()
    except Exception as e:
        return -1, str(e).encode()


def main():
    print("="*70)
    print("PHub HTTP 客户端 PoC")
    print("="*70)
    
    # Step 1: 生成身份
    device_id = generate_device_id("smart-dl-test-001")
    device_sign = generate_device_sign(device_id)
    print(f"\n[1] 身份:")
    print(f"  device_id: {device_id}")
    print(f"  device_sign: {device_sign}")
    
    # Step 2: 构造 PHub QueryRes 请求 (明文,未加密)
    # 用一个测试 infohash
    test_infohash = bytes.fromhex("b773873096c5174a94cc0632d463033b4d46ae50")
    body = build_phub_query_res_request(test_infohash, device_id)
    print(f"\n[2] 构造请求 body ({len(body)} 字节, 未加密):")
    print(f"  hex: {body.hex()}")
    
    # Step 3: 直接发未加密 body (期望被拒, 实测确认服务器期望加密)
    print(f"\n[3] 发送未加密 body 到 {PHUB_HOST}...")
    status, resp = send_phub_request(PHUB_HOST, 80, body, use_https=False)
    print(f"  HTTP {status}")
    print(f"  response: {resp[:200]}")
    
    # Step 4: 用各种 AES key 加密后发送
    print(f"\n[4] 尝试用各种 AES key 加密 body 后发送...")
    aes_keys = [
        ("device_id_md5", hashlib.md5(device_id.encode()).digest()),
        ("appkey_first16", APPKEY.encode()[:16]),
        ("appkey_md5", hashlib.md5(APPKEY.encode()).digest()),
    ]
    for name, key in aes_keys:
        if len(key) != 16:
            continue
        try:
            encrypted = aes_ecb_encrypt(key, body)
            status, resp = send_phub_request(PHUB_HOST, 80, encrypted, use_https=False)
            print(f"  [{name}] key={key.hex()}")
            print(f"    HTTP {status}, response: {resp[:200]}")
        except Exception as e:
            print(f"  [{name}] error: {e}")
    
    # Step 5: 探测 SHub 的 /querybt.fcg?infoid=
    print(f"\n[5] 探测 SHub GET /querybt.fcg?infoid=...")
    try:
        url = f"http://{SHUB_HOST}/querybt.fcg?infoid={test_infohash.hex()}"
        req = urllib.request.Request(url, headers={
            'Host': SHUB_HOST,
            'User-Agent': 'uTorrent',  # 反汇编看到 SHub 用 UA: uTorrent
        })
        r = urllib.request.urlopen(req, timeout=10)
        body = r.read()
        print(f"  [OK] HTTP {r.status}, body ({len(body)} 字节):")
        print(f"    hex: {body[:100].hex()}")
        print(f"    ascii: {body[:100].decode('ascii', errors='replace')}")
    except urllib.error.HTTPError as e:
        body = e.read()
        print(f"  [HTTP {e.code}] body ({len(body)} 字节):")
        print(f"    hex: {body[:100].hex()}")
        print(f"    ascii: {body[:100].decode('ascii', errors='replace')}")
    except Exception as e:
        print(f"  [ERR] {e}")
    
    # Step 6: DCDN 探测
    print(f"\n[6] 探测 DCDN POST /...")
    try:
        url = f"http://{DCDN_HOST}/"
        req = urllib.request.Request(
            url,
            data=b'',
            headers={
                'Host': DCDN_HOST,
                'User-Agent': 'curl/7.64',
                'Content-Type': 'application/octet-stream',
            },
            method='POST',
        )
        r = urllib.request.urlopen(req, timeout=10)
        body = r.read()
        print(f"  [OK] HTTP {r.status}: {body[:200]}")
    except urllib.error.HTTPError as e:
        body = e.read()
        print(f"  [HTTP {e.code}] body ({len(body)} 字节):")
        print(f"    hex: {body[:100].hex()}")
        print(f"    ascii: {body[:100].decode('ascii', errors='replace')}")
    except Exception as e:
        print(f"  [ERR] {e}")


if __name__ == "__main__":
    main()
