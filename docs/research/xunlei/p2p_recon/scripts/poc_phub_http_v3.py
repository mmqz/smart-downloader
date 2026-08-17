"""
PoC v3: 完整 PHub HTTP 客户端 - 正确的 AES key 派生

反汇编证据:
  - 所有 AES 调用都用 XPF_MD5HashData 派生 key
  - 输入: 消息头 8 字节 (offset 0 + offset 5 各 4 字节)
  - 输出: 16 字节 MD5 → AES-128 key
  - 然后用 AES-128-ECB 加密剩余 body

但具体"消息头"是什么仍需推断 — 可能是:
  1. PHub 包头前 8 字节 (PEER_ID 前 4 + 后 4)
  2. PHub 包头 cmd_id (4) + 随机 8 字节
  3. PHub 包头前 4 (cmd) + 4 字节序列号

策略: 先发送各种"前 8 字节 + MD5(8字节) 做 AES key 加密剩余 body"
"""
import struct
import hashlib
import socket
import ssl
import time
import urllib.request, urllib.error
import os
import base64
from pathlib import Path
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from cryptography.hazmat.backends import default_backend

# 沿用 v1/v2 常量
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

PHUB_HOST = "pr-phub.sandai.net"
SHUB_HOST = "hub5btmain.sandai.net"
DCDN_HOST = "dcdnhub-xcloud.sandai.net"


def aes_ecb_encrypt(key, data):
    pad_len = 16 - (len(data) % 16)
    data = data + bytes([pad_len] * pad_len)
    cipher = Cipher(algorithms.AES(key), modes.ECB(), backend=default_backend())
    enc = cipher.encryptor()
    return enc.update(data) + enc.finalize()


def aes_ecb_decrypt(key, data):
    cipher = Cipher(algorithms.AES(key), modes.ECB(), backend=default_backend())
    dec = cipher.decryptor()
    plain = dec.update(data) + dec.finalize()
    if plain and 1 <= plain[-1] <= 16:
        pad_len = plain[-1]
        if plain[-pad_len:] == bytes([pad_len] * pad_len):
            plain = plain[:-pad_len]
    return plain


def md5(data: bytes) -> bytes:
    return hashlib.md5(data).digest()


# 关键算法: AES key 派生
# 反汇编证据 (调用 3, 4, 5, 6, 7, 8 都有):
#   movups xmmword ptr [rsp + 0x30], xmm0       ; 清零 16 字节 output
#   mov eax, dword ptr [rdx + 0]                 ; 读 4 字节 (offset 0)
#   mov dword ptr [rsp + 0x60], eax               ; 存到 input[0:4]
#   mov eax, dword ptr [rdx + 5]                  ; 读 4 字节 (offset 5)
#   mov dword ptr [rsp + 0x64], eax                ; 存到 input[4:8]
#   mov edx, 8                                    ; length = 8
#   call XPF_MD5HashData                          ; MD5(input 8字节) → 16字节 output
#   mov edx, 0x80                                  ; 128 bits = 16 字节
#   call XPF_AESCreateEncryptContext              ; AES-128 key = MD5

def derive_aes_key(header_8bytes: bytes) -> bytes:
    """从消息头 8 字节派生 AES-128 key
    
    反汇编:
      header[0:4] + header[5:9] → 8 字节 input
      MD5(8 字节 input) → 16 字节 AES key
    
    注意: 不是 header[0:8] 而是 header[0:4] + header[5:9]!
    这有点怪 — 可能是 cmd_id (4B) + 4B 字段,跳过某 1 字节字段
    """
    if len(header_8bytes) < 9:
        # 实际需要 9 字节(读 [0:4] + [5:9])
        header_8bytes = header_8bytes.ljust(9, b'\x00')
    part1 = header_8bytes[0:4]   # 偏移 0, 4 字节
    part2 = header_8bytes[5:9]   # 偏移 5, 4 字节 (跳过偏移 4 的 1 字节)
    input_8 = part1 + part2      # 8 字节
    return md5(input_8)          # 16 字节 AES key


def try_phub_with_correct_key_derivation(device_id):
    """用正确的 AES key 派生发请求"""
    print("\n=== PHub: 用 MD5(header[0:4] + header[5:9]) 派生 AES key ===")
    
    # 构造各种 "8字节 header" 候选
    # 候选 1: cmd_id + zero (4B cmd + 1B reserved + 4B seq)
    # 候选 2: peerid 前 8 字节
    # 候选 3: device_id 前 8 字节
    # 候选 4: 时间戳 + ...
    
    test_infohash = bytes.fromhex("b773873096c5174a94cc0632d463033b4d46ae50")
    peerid = b"-XL0019-" + os.urandom(12)
    
    candidates = [
        # (描述, header 9 字节, body 内容)
        ("cmd_id=1 + reserved + seq=0",
         struct.pack("<I", 1) + b'\x00' + struct.pack("<I", 0),  # 9 字节
         b'\x00' * 32),  # body 占位
        ("peerid[0:9]",
         peerid[:9],
         b'\x00' * 32),
        ("device_id[0:9]",
         bytes.fromhex(device_id)[:9],
         b'\x00' * 32),
        ("test_infohash[0:9]",
         test_infohash[:9],
         b'\x00' * 32),
        # 可能 header 不在开头,而是某个内部偏移
        # 用 captcha_sign 前 9 字节
        ("captcha_sign[0:9]",
         b"1.xxxxxx\x00\x00",  # 9 字节
         b'\x00' * 32),
    ]
    
    for name, header9, body in candidates:
        key = derive_aes_key(header9)
        # body 加密
        try:
            encrypted = aes_ecb_encrypt(key, body)
            # 发送
            req = urllib.request.Request(
                f"http://{PHUB_HOST}/",
                data=encrypted,
                headers={
                    'Host': PHUB_HOST,
                    'User-Agent': 'curl/7.64',
                    'Content-Type': 'application/octet-stream',
                },
                method='POST',
            )
            r = urllib.request.urlopen(req, timeout=10)
            resp = r.read()
            print(f"  [{name}] key={key.hex()}")
            print(f"    HTTP {r.status}, resp: {resp[:200]}")
        except urllib.error.HTTPError as e:
            resp = e.read()
            print(f"  [{name}] key={key.hex()}")
            print(f"    HTTP {e.code}, resp: {resp[:200]}")
        except Exception as e:
            print(f"  [{name}] err: {e}")


def try_dcdn_with_correct_key_derivation(device_id):
    """DCDN: 用正确的 AES key 派生 + base64 包装"""
    print("\n=== DCDN: MD5 派生 key + base64 包装 ===")
    
    # DCDN 返回 "401 Decode error" 说明它期望 base64
    # 但 base64 内是 AES-ECB 加密的数据
    
    # 试几种 header
    test_bodies = [
        # (描述, header 9 字节, body 内容)
        ("empty",
         b'\x00' * 9,
         b'\x00' * 32),
        ("device_id[0:9]",
         bytes.fromhex(device_id)[:9],
         b'\x00' * 32),
        ("cmd_ping",
         struct.pack("<I", 0) + b'\x00' + struct.pack("<I", 0),
         b'\x00' * 32),
    ]
    
    for name, header9, body in test_bodies:
        key = derive_aes_key(header9)
        try:
            encrypted = aes_ecb_encrypt(key, body)
            # base64 包装
            encoded = base64.b64encode(encrypted)
            req = urllib.request.Request(
                f"http://{DCDN_HOST}/",
                data=encoded,
                headers={
                    'Host': DCDN_HOST,
                    'User-Agent': 'curl/7.64',
                    'Content-Type': 'application/octet-stream',
                },
                method='POST',
            )
            r = urllib.request.urlopen(req, timeout=10)
            resp = r.read()
            print(f"  [{name}] key={key.hex()}")
            print(f"    HTTP {r.status}, resp: {resp[:200]}")
        except urllib.error.HTTPError as e:
            resp = e.read()
            print(f"  [{name}] key={key.hex()}")
            print(f"    HTTP {e.code}, resp: {resp[:200]}")
        except Exception as e:
            print(f"  [{name}] err: {e}")


def main():
    print("="*70)
    print("PoC v3: 正确 AES key 派生 (MD5(header[0:4]+header[5:9]))")
    print("="*70)
    
    device_id = hashlib.md5(b"smart-dl-test-001").hexdigest()
    print(f"\n[1] device_id: {device_id}")
    
    try_phub_with_correct_key_derivation(device_id)
    try_dcdn_with_correct_key_derivation(device_id)
    
    print("\n" + "="*70)
    print("结论:")
    print("  - PHub 仍返回 'decrypt request failed' = AES key 不对")
    print("  - DCDN 仍返回 '401 Decode error' = base64/AES 不对")
    print("  - 说明 header 8 字节不是简单的前 9 字节")
    print("  - 可能是 PHub 包头内部某偏移的字段,需要更深入反汇编")
    print("  - 或者 PHub 包头本身就需要先用固定 key 加密,再嵌套")
    print("="*70)


if __name__ == "__main__":
    main()
