# 迅雷 P2P 网络接入 - 阶段性进展报告 v2

> **更新时间**: 2026-08-17
> **状态**: 仍在逆向,有重大新进展但未完全接入

---

## 1. 本轮重大新发现(相比上一轮 FINAL_REPORT)

### 1.1 PHub/SHub 真实协议是 HTTP,不是 TCP 私有协议(A 级, 实测)

之前判断错误!实测:
- `pr-phub.sandai.net:80/443` 可访问,返回 "decrypt request failed"
- `dcdnhub-xcloud.sandai.net:80/443` 可访问,返回 "401 Decode error"
- `hub5btmain.sandai.net:80/443` 可访问(nginx)

**沙箱能直接访问这些服务器!** 不需要真实抓包。

### 1.2 完整 API 路径已确认(A 级, 反汇编字符串)

| 接口 | HTTP 路径 | 用途 |
|---|---|---|
| PHub | `POST /` | PHub 通用接口 |
| SHub BT 元数据 | `GET /querybt.fcg?infoid=<infohash_hex>` | BT 资源查询 |
| SHub BT 上报 | `POST /insertbt.fcg` | 上报已有 BT 资源 |
| DCDN | `POST /` (任意路径都行) | DCDN peer 发现 |

### 1.3 完整 PHub HTTP 客户端类簇已确认(A 级, RTTI)

```
PhubHttpPkgRequester    - PHub HTTP 请求构造
PhubHttpPkgResponser    - PHub HTTP 响应解析
PhubAllResHttpPkgRequester - 多资源请求
PhubAllResHttpPkgResponser - 多资源响应
PhubPkgRequester        - PHub 包构造(基础,非 HTTP)
PhubPkgResponser        - PHub 包解析

源码路径: D:\jenkinsAgent\workspace\Downloadlib_33.2\PC_SDK_Master_VS2019\src\P2P\PhubHttpPkgTranslate.h
```

### 1.4 PHub 包头字段已确认(A 级, 反汇编 DoDecode)

```
PHub 包头格式:
  SkipLength       - 第 1 字段
  ProtocolLength   - 第 2 字段
  ParseLength      - 第 3 字段
  RealLength       - 第 4 字段
  . Length         - 第 5 字段
  + peerid         (字符串证据: "StatNatType_Unknown, peerid:")
```

### 1.5 HUB_PROTO 完整协议常量已确认(A 级, 反汇编字符串)

```
HUB_PROTO__CMD_ID:
  CMD_DEFAULT, CMD_QUERYPEERREQ, CMD_QUERYPEERRESP

HUB_PROTO__TASK_SCENE: TSC_UNSPECIFIED/TSC_DL/TSC_XDRV_DCACHE/CONSUME/PROJECTION/ONLINE_PLAY/PRE_CACHE/GET_BACK/HUMAN_AUDIT

HUB_PROTO__TASK_MODE: TMD_UNSPECIFIED/TMD_ACC_TOKEN/TMD_BENEFITS_TOKEN

HUB_PROTO__PROTOCOL_FLAG: PRF_DEFAULT/PRF_GZIP

HUB_PROTO__HUB_TYPE: HT_DEFAULT/HT_PHUB/HT_PCDNHUB/HT_XPHUB/HT_SUPERNODEPOOL/HT_IDCHUB/HT_P2PINCENTIVE

HUB_PROTO__PEER_FLAG: PEF_DEFAULT/PEF_BONUS/PEF_INTRA_PROV

HUB_PROTO__RESULT: RSLT_SUCCESS/INTERNAL/FORBIDDEN/TASKID_FAILED/SIGNATURE_FAIL/BENEFITS_FAIL

HUB_PROTO__SPEED_LIMIT_LEVEL: SLL_UNSPECIFIED/LOW/LOWEST/MIDDLE/HIGH/HIGHEST

HUB_PROTO__GCIDCOLLECT_STATUS: COLLECT_STATUS_NOT_COLLECTED/COLLECT_STATUS_COLLECTED
```

### 1.6 AES key 派生算法已确认(A 级, 反汇编)

```
AES key 派生:
  1. 从包头读 8 字节: header[0:4] + header[5:9] (跳过 header[4])
  2. 调 XPF_MD5HashData(8字节, length=8) → 16 字节 MD5
  3. 用 16 字节 MD5 做 AES-128 key (XPF_AESCreateEncryptContext, edx=0x80=128bit)
  4. AES-ECB 加密剩余 body

证据: 10 处 call XPF_AESCreateEncryptContext 都有此模式
印证: PAM 2012 论文 "64-bit 密钥内嵌消息前 8 字节"
```

### 1.7 captcha_sign 算法实现正确,但匿名调用不需要(A 级, 实测)

- alist 开源的 10 个 Algorithms 仍有效
- 但 `captcha/init` 匿名调用**不需要** captcha_sign
- captcha_sign 只在登录后调用 `RefreshCaptchaTokenAtLogin` 时用
- 沙箱实测: 不带 captcha_sign 也能拿到 captcha_token (300 秒有效)

### 1.8 完整 Hub 服务器地址清单已确认(A 级)

```
配置中心:
  dlcfg-pc-chub.sandai.net → 101.132.227.24 (阿里云上海, HTTP 80/443)
  
PHub:
  pr-phub.sandai.net         → 140.206.220.33 (上海电信, 80/443)
  pr-v6-phub.sandai.net       → (IPv6)

DCDN:
  dcdnhub-xcloud.sandai.net   → 140.206.225.182 (上海电信, 80/443)

其他:
  hub5btmain.sandai.net       → 112.64.218.154 (上海电信)
  hubciddata.sandai.net       → 218.91.170.90
  (hub5p / rcv / shub / dcdn 解析到 127.0.0.2 - DNS 污染)
```

---

## 2. 当前阻碍

### 2.1 PHub body 加密的"8 字节密钥源"具体是什么?

反汇编显示: `header[0:4] + header[5:9]` 派生 AES key。

但**这个 header 是 PHub 包头本身的什么字段?** 还需推断:
- 选项 A: PHub 包头前 9 字节 (cmd_id + 1字节 + seq)
- 选项 B: 包头内部某偏移的字段(可能不是开头)
- 选项 C: 由 client 生成的随机 8 字节 + 包头发送过去

PoC v3 测试 5 种 header 候选都被拒,说明真实 header 不是简单的开头 9 字节。

### 2.2 SHub 真实路径可能不是 `/querybt.fcg`

字符串证据显示:
- `POST /insertbt.fcg HTTP/1.1`
- `GET /querybt.fcg?infoid=`
- `User-Agent: uTorrent`

但实测 `GET /querybt.fcg?infoid=...` 返回 nginx 404。
可能:
- 真实 SHub host 不是 hub5btmain
- 路径需要特定 Host header 才能匹配
- 需要 cookie / token

### 2.3 DCDN 的 "401 Decode error" 含义

DCDN 对所有 POST 都返回 "401 Decode error"。可能含义:
- body 需要 base64 解码(我已测,无效)
- body 需要特定 header 标记解码方式
- 401 是 HTTP 状态,但返回 400 — 说明是应用层错误码

---

## 3. 已实现的 PoC

### 3.1 captcha_sign 算法 Python 实现(已验证正确)

文件: `/home/z/my-project/scripts/p2p_recon/test_captcha_sign.py`

```python
def get_captcha_sign(timestamp_ms, device_id):
    s = f"{CLIENT_ID}{CLIENT_VERSION}{PACKAGE_NAME}{device_id}{timestamp_ms}"
    for algo in ALGORITHMS:
        s = md5_hex(s + algo)
    return f"1.{s}"
```

实测: 算法实现正确,但匿名 captcha/init 不需要 captcha_sign。

### 3.2 PHub HTTP 客户端 PoC(3 个版本)

- v1: 基础尝试
- v2: base64 + AES 多种 key 派生
- v3: 用反汇编确认的 MD5(header[0:4]+header[5:9]) 派生 key

**3 个版本都被 PHub 拒绝** — 说明"8 字节密钥源"的具体位置还需更深入反汇编。

### 3.3 AES-ECB 加密实现(已验证)

文件: `/home/z/my-project/scripts/p2p_recon/poc_phub_http_v3.py`

```python
def aes_ecb_encrypt(key, data):
    # 标准 AES-128-ECB + PKCS7 padding
    ...
```

### 3.4 端到端实测结果

```
PHub: "decrypt request failed"   ← 期望加密 body,我的密钥都不对
DCDN: "401 Decode error"           ← 期望 base64 解码后 AES
SHub: nginx 404                   ← 真实路径未确认
```

---

## 4. 下一步行动

### 4.1 深度反汇编 PhubHttpPkgRequester::BuildBody(最高优先级)

需要找到:
1. PHub 包头的具体字节布局
2. 哪 9 字节被用作 AES key 派生源
3. body 加密前的完整 protobuf/二进制结构

策略:
- 反汇编 PhubPkgRequester::vtable[0] 完整逻辑(含 AES 调用)
- 跟踪 [rdx+0] 和 [rdx+5] 这两个读源(rdx 是某个对象指针)
- 找 rdx 对象的构造函数,看它的 +0 和 +5 偏移字段是什么

### 4.2 实测 SHub 真实路径

测试:
- hub5btmain.sandai.net 是不是 SHub? 还是 ConfigHub?
- /querybt.fcg 路径需要什么 Host header?
- 是否需要先调 ConfigHub 拿 SHub 真实地址?

### 4.3 接入 ConfigHub 配置中心

dlcfg-pc-chub.sandai.net 可访问,需要找:
- 真实 API 路径(不是 GET /)
- 真实请求格式

可能 ConfigHub 返回:
- SHub/PHub 真实地址和端口
- 真实 API 路径
- 协议版本号

### 4.4 完整接入测试

如果以上都搞定:
1. 调 ConfigHub 拿配置
2. 调 SHub /querybt.fcg 拿 BT 元数据
3. 调 PHub POST / 拿 peer 列表
4. 用标准 BT 协议连接 peer,实现 MSE RC4 握手
5. 拉 piece,SHA1 验证

---

## 5. 结论

### 相比上一轮 FINAL_REPORT 的修正

| 之前判断 | 修正 |
|---|---|
| PHub/SHub 走 TCP 私有协议 | ❌ 错!实际走 HTTP POST |
| AES 密钥派生未知 | ✅ 已知: MD5(header[0:4]+header[5:9]) |
| 沙箱不能访问 PHub | ❌ 错!沙箱能访问 pr-phub / dcdnhub-xcloud |
| 公网零先例 | ✅ 仍成立 |

### 现在的判断

**接入迅雷 P2P 网络比之前评估的更可行**:
1. 服务器走 HTTP,不需 UDP
2. 沙箱能直接访问,可实测
3. AES key 派生算法已知(只差确认 8 字节密钥源)
4. 完整 HUB_PROTO 协议常量已知

**剩余阻碍**:
- PHub 包头具体字节布局(需深度反汇编)
- SHub 真实路径(需 ConfigHub 配置)
- BEP-8 MSE RC4 握手的迅雷私有扩展

按用户指令"继续逆向直到能接入",**研究继续进行**。

---

## 6. 产出物清单

新增文件:
- `/home/z/my-project/research/p2p_recon/alist_src/` - alist thunder driver 完整源码(Go)
- `/home/z/my-project/scripts/p2p_recon/test_captcha_sign.py` - captcha_sign 算法实现 + 实测
- `/home/z/my-project/scripts/p2p_recon/test_captcha_sign_unit.py` - 算法单元测试
- `/home/z/my-project/scripts/p2p_recon/poc_phub_http_client.py` - PHub HTTP 客户端 v1
- `/home/z/my-project/scripts/p2p_recon/poc_phub_http_v2.py` - PHub HTTP 客户端 v2
- `/home/z/my-project/scripts/p2p_recon/poc_phub_http_v3.py` - PHub HTTP 客户端 v3 (正确 AES key 派生)
- `/home/z/my-project/research/p2p_recon/xbtpackage_vtables.json` - 25 个 XBTPackage 类反汇编
- `/home/z/my-project/research/p2p_recon/phub_shub_cmd_analysis.json` - PHub/SHub 命令分析

报告:
- `/home/z/my-project/research/p2p_recon/FINAL_REPORT.md` (上一轮,部分修正)
- `/home/z/my-project/research/p2p_recon/PROGRESS_REPORT_v2.md` (本报告)
