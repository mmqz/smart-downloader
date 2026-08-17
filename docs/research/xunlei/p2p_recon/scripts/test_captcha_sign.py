"""
迅雷云盘 API Python 客户端 - 验证 captcha_sign + device_sign 算法

目标:
1. 用 Python 复现 alist Go 的 captcha_sign 算法
2. 实测能否匿名调用 captcha/init 拿 captcha_token
3. (如有账号) 测试登录链路: core login → captcha init → signin token
4. 验证 Algorithms 10 个盐是否仍有效

⚠ 这是测试 captcha_sign 算法是否仍有效,不涉及任何 P2P 协议
   云盘 API 是公开 HTTPS 接口,合法可访问

参考: alist drivers/thunder/util.go (Go 实现,MIT 许可)
"""
import hashlib
import json
import time
import urllib.request
import urllib.error
import socket

# ============= 迅雷客户端身份 (来自 alist, 已开源) =============
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

# 10 个 captcha_sign 盐 (alist 已开源, MIT 许可)
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


def md5_hex(s: str) -> str:
    """MD5 hex digest"""
    return hashlib.md5(s.encode('utf-8')).hexdigest()


def sha1_hex(s: str) -> str:
    """SHA1 hex digest"""
    return hashlib.sha1(s.encode('utf-8')).hexdigest()


def get_captcha_sign(timestamp_ms: str, device_id: str) -> str:
    """计算 captcha_sign
    
    算法 (来自 alist):
      str = ClientID + ClientVersion + PackageName + DeviceID + timestamp
      for algo in Algorithms:
          str = md5(str + algo)
      return "1." + str
    """
    s = f"{CLIENT_ID}{CLIENT_VERSION}{PACKAGE_NAME}{device_id}{timestamp_ms}"
    for algo in ALGORITHMS:
        s = md5_hex(s + algo)
    return f"1.{s}"


def generate_device_sign(device_id: str) -> str:
    """计算 device_sign
    
    算法 (来自 alist):
      base = DeviceID + PackageName + APPID + APPKey
      sha1 = SHA1(base) hex
      md5 = MD5(sha1) hex
      return "div101." + DeviceID + md5
    """
    base = f"{device_id}{PACKAGE_NAME}{APPID}{APPKEY}"
    sha1 = sha1_hex(base)
    md5 = md5_hex(sha1)
    return f"div101.{device_id}{md5}"


def generate_device_id(seed: str = "smart-dl-v1") -> str:
    """生成 32 字节 hex device_id (与迅雷 alist 一致)"""
    if len(seed) == 32 and all(c in '0123456789abcdef' for c in seed.lower()):
        return seed
    return md5_hex(seed)


def captcha_init(action: str, device_id: str, meta: dict = None) -> dict:
    """调用 /v1/shield/captcha/init
    
    匿名调用,只需 device_id + captcha_sign
    返回 captcha_token (有效期 300 秒)
    """
    timestamp_ms = str(int(time.time() * 1000))
    captcha_sign = get_captcha_sign(timestamp_ms, device_id)
    
    metas = meta or {}
    metas["timestamp"] = timestamp_ms
    metas["captcha_sign"] = captcha_sign
    
    body = {
        "action": action,
        "captcha_token": "",
        "client_id": CLIENT_ID,
        "device_id": device_id,
        "meta": metas,
        "redirect_uri": "xlaccsdk01://xunlei.com/callback?state=harbor",
    }
    
    req = urllib.request.Request(
        "https://xluser-ssl.xunlei.com/v1/shield/captcha/init",
        data=json.dumps(body).encode('utf-8'),
        headers={
            'Content-Type': 'application/json;charset=UTF-8',
            'User-Agent': USER_AGENT,
            'x-device-id': device_id,
            'x-client-id': CLIENT_ID,
            'x-client-version': CLIENT_VERSION,
            'accept': 'application/json;charset=UTF-8',
        },
        method='POST',
    )
    
    socket.setdefaulttimeout(15)
    try:
        r = urllib.request.urlopen(req, timeout=15)
        return json.loads(r.read().decode('utf-8'))
    except urllib.error.HTTPError as e:
        return {
            "error": f"HTTP {e.code}",
            "body": e.read().decode('utf-8', errors='ignore'),
        }


def main():
    print("="*70)
    print("迅雷云盘 API captcha_sign 算法验证")
    print("="*70)
    
    # Step 1: 生成 device_id + device_sign
    device_id = generate_device_id("smart-dl-test-device-001")
    device_sign = generate_device_sign(device_id)
    
    print(f"\n[1] device_id: {device_id}")
    print(f"    device_sign: {device_sign}")
    
    # Step 2: 计算一个示例 captcha_sign (不调服务器,只算)
    timestamp_ms = str(int(time.time() * 1000))
    captcha_sign = get_captcha_sign(timestamp_ms, device_id)
    print(f"\n[2] 示例 captcha_sign 计算:")
    print(f"    timestamp: {timestamp_ms}")
    print(f"    captcha_sign: {captcha_sign}")
    
    # Step 3: 实测匿名调 captcha/init
    print(f"\n[3] 实测 captcha/init (匿名):")
    # action 是 GET:/user/me (自检接口)
    result = captcha_init(
        action="GET:/v1/user/me",
        device_id=device_id,
    )
    print(f"    result: {json.dumps(result, indent=2, ensure_ascii=False)}")
    
    if "captcha_token" in result:
        print(f"\n✅ captcha_sign 算法有效!")
        print(f"   captcha_token: {result['captcha_token'][:60]}...")
        print(f"   expires_in: {result.get('expires_in', 'N/A')}")
        print(f"\n   说明: alist 开源的 Algorithms 在 v8.31.0.9726 仍可用")
        print(f"   现在可以继续走登录链路 (需账号) 或调云盘 API (需登录)")
    else:
        print(f"\n❌ captcha_sign 失效或返回错误")
        print(f"   可能原因: Algorithms 已被新版迅雷更换")
        return
    
    # Step 4: 看是否需要 review_panel
    if result.get("url"):
        print(f"\n⚠ 触发 review_panel (需要验证码)")
        print(f"   url: {result['url']}")


if __name__ == "__main__":
    main()
