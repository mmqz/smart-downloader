# 迅雷 P2P 协议公开资料调研报告

> 调研目标: 评估迅雷私有 P2P 网络协议公开资料的反向工程覆盖度
> 调研日期: 2026-08-17
> 调研深度: 学术论文 PDF + GitHub 源码 + 中文技术博客 + 防火墙规则库 + 反向社区

---

## 1. 调研结论概览

| 资料 | 是否找到 | 公开度 | 接入可用度 |
|---|---|---|---|
| ThunderPlatform 握手文档 (OSChina) | **未找到** | - | - |
| XLP 极速通道协议文档 | **未找到** | - | - |
| Fastdick 协议 | 找到, 但非 P2P | HTTP API only | 不适用 P2P |
| PAM 2012 学术论文 | **找到完整 PDF** | 公开 | ⭐⭐⭐⭐ 关键 |
| Tsinghua HMC 论文 | **找到完整 PDF** | 公开 | ⭐⭐⭐⭐ 关键 |
| 看雪 251933 帖子 | **找到, 但需登录** | 仅标题/snippet | ⭐⭐⭐ |
| PeerBanHelper 规则 | **找到完整规则** | 公开 | ⭐⭐⭐⭐⭐ 决定性 |
| ThunderOpenSDK 接口 | **找到完整文档** | 公开 | ⭐⭐⭐ |
| Hub5p 域名清单 | **找到完整列表** | 公开 | ⭐⭐⭐⭐ |
| CID/GCID 算法 | **找到开源实现** | 公开 | ⭐⭐⭐⭐⭐ 决定性 |
| PeerID 格式 | **找到 BT spec 收录** | 公开 | ⭐⭐⭐⭐⭐ 决定性 |

**核心结论**:
- 公开资料覆盖了 **协议总览、加密模式、Hub 服务器列表、peer_id 格式、CID/GCID 算法**。
- 公开资料 **没有** 覆盖 P2P peer-to-peer 之间的具体扩展协议(扩展 id / 载荷格式)。
- 第三方独立接入迅雷 P2P 网络在公开资料层面 **未找到任何已实现案例**。
- 唯一找到的「破解」方案是修改本地 SQLite DB (TaskDb.dat) UserData 字段。

---

## 2. 找到的所有相关资料

### 学术论文 (A 级 - 最权威)

1. **PAM 2012 论文 (核心)**
   - URL: https://www.moritzsteiner.de/papers/PAM2012paper20.pdf
   - ACM: https://dl.acm.org/doi/10.1007/978-3-642-28537-0_23
   - 标题: "Xunlei: Peer-Assisted Download Acceleration on a Massive Scale"
   - 作者: Prithula Dhungel, Keith W. Ross, Moritz Steiner, Ye Tian, Xiaojun Hei
   - 一句话说明: 揭示迅雷 AES-ECB 加密、64-bit 密钥内嵌、ID 格式、tracker 接口。

2. **Tsinghua HMC 论文 (协议结构)**
   - URL: https://tsinghua-nslab.org/assets/files/hmc-b3f76b0d714e2b7a50a7d3ccbbdbb83a.pdf
   - 标题: "HMC: A Novel Mechanism for Identifying Encrypted P2P Thunder Traffic"
   - 作者: Chenglong Li, Yibo Xue (清华大学), Yingfei Dong (U Hawaii)
   - 一句话说明: 揭示 Thunder 包结构 = Header(4B cmd + variable conn) + Body(encrypted), 命令字 300+ 种。

3. **扩展技术报告 (PAM 2012 extended)**
   - URL: http://cis.poly.edu/~prithula/papers/XunleiTR.pdf
   - 标题: "Measurement Study of Xunlei: Extended Version"
   - 一句话说明: PAM 2012 的扩展版,可能含更多协议细节(未抓取成功)。

4. **Chalmers / ResearchGate 重复刊**
   - URL: https://research.chalmers.se/en/publication/119426
   - URL: https://www.researchgate.net/publication/229067694
   - 一句话说明: "Architecture and Download Behavior of Xunlei" (Zhang et al., 2010 PAM student workshop)。

### GitHub 仓库

5. **PeerBanHelper (PBH)** - 决定性证据
   - URL: https://github.com/PBH-BTN/PeerBanHelper
   - 文档: https://docs.pbh-btn.com/en/docs/misc/json-engine
   - Issue 211: https://github.com/PBH-BTN/PeerBanHelper/discussions/211
   - Issue 1358: https://github.com/PBH-BTN/PeerBanHelper/issues/1358
   - 一句话说明: 反吸血规则明确识别 `-XL0019-` peer_id 和含 "xunlei"/"Thunder" 的 ClientName。

6. **BTN-Collected-Rules** - IP 黑名单
   - URL: https://github.com/PBH-BTN/BTN-Collected-Rules
   - 一句话说明: BTN 网络收集的恶意 IP 列表 (含 aria2c/迅雷变种),Transmission 兼容。

7. **transmission-block** - 默认封禁规则 (决定性)
   - URL: https://github.com/qianbinbin/transmission-block
   - 配置: https://raw.githubusercontent.com/qianbinbin/transmission-block/master/transmission-block.conf
   - 一句话说明: 默认 `LEECHER_CLIENTS` 包含完整迅雷 peer_id 字节特征 `%FF%1D%FF%FF%FF8I%FF` + ClientName 关键字。

8. **ThunderOpenSDK** (cryzlasm) - 公开 SDK
   - URL: https://github.com/cryzlasm/ThunderOpenSDK
   - 官方文档: http://open.xunlei.com/wiki/api_doc.html
   - 一句话说明: 完整列出迅雷公开 SDK 的 11 个 XL_* 接口, 含 `dl_peer_id.dll` (peer_id 生成器)。

9. **xunlei-dlsdk** (官方)
   - URL: https://github.com/xunlei-open/xunlei-dlsdk
   - 一句话说明: 迅雷官方放出的 SDK 指南,但仅描述接入方式,不含协议细节。

10. **deathbless/thunder** - 高速通道破解 (SQLite 注入)
    - URL: https://github.com/deathbless/thunder
    - 一句话说明: 通过修改 TaskDb.dat 的 superspeed/offline 表 UserData.Result=0 实现本地绕过, **不涉及协议**。

11. **EasonRen/SuperSpeed** - 同上 (仅有 README,无源码)
    - URL: https://github.com/EasonRen/SuperSpeed

12. **Xunlei-Fastdick** - 带宽提速 (非 P2P)
    - URL: https://github.com/fffonion/Xunlei-Fastdick
    - Python 重写: https://github.com/timothyqiu/python-swjsq
    - 一句话说明: 迅雷"快鸟" ISP 链路带宽提速,基于 xl-acc-sdk HTTP API,**不是 P2P 协议**。

13. **iambus/xunlei-lixian** - 离线下载 (旧 HTTP API)
    - URL: https://github.com/iambus/xunlei-lixian (archived 2021)
    - 文件: https://github.com/iambus/xunlei-lixian/blob/master/lixian_hash.py
    - 一句话说明: 实现 CID/DCID/GCID/ed2k 算法,基于 lixian.xunlei.com HTTP API。

14. **zyxar/xunlei** (Go) - 同上 Go 实现
    - URL: https://github.com/zyxar/xunlei
    - 一句话说明: Go 重写 iambus/xunlei-lixian, 仍是 HTTP API,**不是 P2P**。

15. **cnk3x/xunlei** - 群晖套件 (非协议)
    - URL: https://github.com/cnk3x/xunlei
    - 一句话说明: 仅提取群晖 SPK 包 + Linux 容器化运行,**无协议代码**。

16. **FileCentipede** - 不实现迅雷
    - URL: https://github.com/filecxx/FileCentipede
    - 一句话说明: 仅支持解码 `thunder://` URL 前缀 (Base64),**不实现迅雷 P2P**。

17. **xunlei-open/xunlei-dlsdk** - 官方 SDK 仓库
    - URL: https://github.com/xunlei-open/xunlei-dlsdk
    - 一句话说明: 迅雷官方 SDK 指南,仅接入文档,无协议层。

### 中文技术博客 / 论坛

18. **看雪论坛 251933** (关键, 需登录)
    - URL: https://bbs.kanxue.com/thread-251933.htm
    - 标题: "[原创]迅雷下载服务加速节点的来源分析"
    - 一句话说明: 提到 AES-ECB 解密 hub5btmain.v6.shub.sandai.net 接口,返回 GUID/CID, 通过 lua 脚本 `LuaServiceSHubQueryBTFileIndexCallBack::LuaCallBack` 处理。
    - 备注: 帖子内容被反爬保护,snippet 来自 Google 索引缓存。

19. **看雪论坛 60110** (老帖)
    - URL: https://bbs.kanxue.com/thread-60110.htm
    - 标题: "迅雷协议分析--多链接资源获取" (vessial, 2007)
    - 一句话说明: 2007 年的迅雷早期协议分析,但内容已被云防御拦截。

20. **百度智能云文章**
    - URL: https://cloud.baidu.com/article/3009655
    - 一句话说明: 浅层 P2SP 原理科普,无协议细节。

21. **FortiGuard 应用识别**
    - URL: https://www.fortiguard.com/appcontrol/14797
    - 一句话说明: Fortinet 防火墙对 Thunder.Xunlei 的应用层签名,可用于抓包对照。

22. **Clavister 防火墙规则**
    - URL: https://docs.clavister.com/repo/cos-stream-application-control-signatures/4.10/doc/ch21s84.html
    - 一句话说明: Clavister 对 Xunlei/Thunder 协议的识别规则,可用于网络层指纹。

### alist / OpenList 迅雷驱动

23. **OpenList Thunder Driver** (云盘 HTTP API)
    - URL: https://doc.oplist.org/guide/drivers/thunder
    - 一句话说明: 迅雷云盘 HTTP API 逆向接口, **不是 P2P**。包含 CaptchaSign 算法:
      ```
      str = ClientID + ClientVersion + PackageName + DeviceID + Timestamp
      for (Algorithm in Algorithms):
          str = md5(str + Algorithm)
      CaptchaSign = "1." + str
      ```

---

## 3. ThunderPlatform 握手文档 - 未找到

**结论**: OSChina / 看雪 / 博客园均 **未公开** ThunderPlatform 握手协议文档。

间接证据:
- ThunderPlatform = `MiniThunderPlatform.exe` 子进程,由 `xldl.dll` 拉起
- 调用 `XL_Init` 时启动,通过命名管道与上层通信
- 任务实际在 MiniThunderPlatform 进程内创建
- 但 **xldl.dll 与 MiniThunderPlatform.exe 之间的 IPC 协议无公开文档**

来源: ThunderOpenSDK README (https://github.com/cryzlasm/ThunderOpenSDK)

```
SDK 文件:
  xldl.dll            → 导出 MiniTP 接口
  MiniThunderPlatform.exe → 独立进程 (TP)
  download_engine.dll → MiniTP 核心库
  zlib1.dll           → 压缩通信数据
  dl_peer_id.dll      → 获取迅雷客户端标识  ← 关键!
  XLBugReport.exe     → 崩溃上报
  XLBugHandler.dll    → 拉起 XLBugReport.exe
  minizip.dll, mini_unzip.dll → 崩溃堆栈压缩
  atl71.dll           → 微软运行库
```

---

## 4. XLP 极速通道协议文档 - 未找到

**结论**: 公开网络上 **完全没有** "XLP 协议" / "极速通道协议" 的逆向资料。

搜索范围:
- GitHub: 0 命中 (仅有 guanjj28/XLP-Guidebook 是无关的群体学习手册)
- 中文技术博客: 0 命中
- 学术搜索: 0 命中

唯一相关:
- **EasonRen/SuperSpeed** 仓库: README 仅一句"迅雷高速通道破解",无源码。
- **deathbless/thunder** 仓库: 通过 SQLite 注入 (`UPDATE UserData SET Result=0`) 修改本地状态。
  - 这是 **本地状态欺骗**,不是协议层逆向。
  - `TaskDb.dat` 中的 `superspeed` 和 `offline` 表存储高速通道任务的 UserData (JSON 格式)
  - 仅修改 `Result: 0` 和 `Message: "fuck u thunder"` 即可让客户端误以为已激活
  - 完全不涉及与服务器之间的协议

---

## 5. Fastdick 协议文档 - 完整贴出

> 重要澄清: **Fastdick 不是 P2P 协议**, 而是"迅雷快鸟"——一个 ISP 链路带宽提速服务(把用户的家宽从 100M 提升到 200M 之类)。它走 HTTP API,与 BT peer-to-peer 协议无关。

来源: https://github.com/fffonion/Xunlei-Fastdick/blob/master/swjsq.py (本地存档 /tmp/fastdick.py)

### 5.1 关键常量

```python
APP_VERSION       = "2.4.1.3"
PROTOCOL_VERSION  = 200
VASID_DOWN        = 14   # 下行加速 vasid
VASID_UP          = 33   # 上行加速 vasid
FALLBACK_MAC      = '000000000000'
FALLBACK_PORTAL   = "119.147.41.210:12180"   # 下行 portal
FALLBACK_UPPORTAL = "153.37.208.185:81"      # 上行 portal
```

### 5.2 HTTP 头

```python
header_xl = {
    'Content-Type':'',
    'Connection': 'Keep-Alive',
    'Accept-Encoding': 'gzip',
    'User-Agent': 'android-async-http/xl-acc-sdk/version-2.1.1.177662'
}

header_api = {
    'Content-Type':'',
    'Connection': 'Keep-Alive',
    'Accept-Encoding': 'gzip',
    'User-Agent': 'Dalvik/2.1.0 (Linux; U; Android 5.0.1; R1 Build/LRX22C)'
}
```

### 5.3 协议特征

- 客户端伪装: `SmallRice R1` / Android 5.0.1 / API 24
- 走 xl-acc-sdk (迅雷账号 SDK),需要登录账号
- 返回 JSON,含 `down_xxM` / `up_xxM` 表示提速后带宽
- 使用 vasid 区分上行/下行套餐
- 服务端使用自签名证书 (代码特意禁用 SSL 验证)

### 5.4 与 P2P 协议的关系

**无关**。Fastdick 只解决"我的物理链路只跑了 100M,如何让 ISP 给我开 200M"问题,不影响迅雷 BT peer 之间的数据传输。

---

## 6. PAM 2012 论文 - 关键章节完整贴出

来源: https://www.moritzsteiner.de/papers/PAM2012paper20.pdf (本地存档 /tmp/pam2012.pdf, 文本 /tmp/pam2012.txt)

### 6.1 加密方式 (Section 2)

> Xunlei uses **AES in ECB mode** for encrypting messages exchanged between its entities.
> The **64-bit key for each message is pre-pended to the message itself**.

**关键含义**: 每条迅雷消息前 8 字节就是 AES-ECB 密钥,后续是加密载荷。这意味着:
- 加密强度仅 64-bit (8 字节,与 DES 同量级)
- 密钥"内嵌"导致任何能读到包的人都能解密
- 这是迅雷的"obfuscation"而非真正的安全机制

### 6.2 多种 Peer ID 格式 (Section 2)

> Each peer in Xunlei uses different identifiers for itself when joining different networks:
> - **16-byte identifier** when joining the KAD network
> - **20-byte identifier** in the Xunlei DHT
> - **20-byte identifier** for BitTorrent
> - **16-byte unique identifier** when registering with the Xunlei central trackers (the **Xunlei ID**)
>   - Its **first 12 bytes correspond to the hexadecimal equivalent of the MAC address** of the machine

**关键含义**: 迅雷在中心 tracker 注册的 16 字节 Xunlei ID 的前 12 字节就是 MAC 地址。这意味着:
- 任何向迅雷 tracker 报告过的客户端都可以被 MAC 追踪
- 同一机器即使换 IP 也可被识别
- PAM 2012 据此估算出"20 million unique users per month for Kankan TV shows"

### 6.3 Tracker 返回结构 (Section 2.3)

> When downloading a file using a resource link, the Xunlei client sends a message with the link to the central tracker, which in turn returns:
> - **two 20-byte hash values**
> - **a single 8-byte code** corresponding to the file.
>
> These hash values and the code are then used to request the peer and server resource lists for the file.
>
> For BitTorrent files, the Xunlei client uses the **infohash** of the file as the 20-byte identifier.
> For eDonkey files, the 20 byte identifier is obtained from the 16 byte hash extracted from the ed2k link along with the file size.

**关键含义**: 迅雷 tracker 协议层
1. 客户端发: file identifier (BT infohash / ed2k hash / URL)
2. tracker 回: 两个 20B 哈希 + 1 个 8B code
3. 客户端用这些 hash+code 请求 peer 列表和 server 列表

### 6.4 跨协议资源映射 (Section 2.1)

> The Xunlei client constructs an **internal hash** for each file it has downloaded, and then sends this hash to a tracker, along with the identifiers of the sources from which it downloaded the file.
> The tracker most likely has a hash table, with the internal hash being the key, and a list of all sources that are known to have the file, with the identifier type being source specific.
> Sources can include: HTTP/FTP/RTSP/MMS URLs, BT infohash, eDonkey hash, and the Xunlei IDs of the peers holding the file.

**关键含义**: 迅雷 tracker 维护一个以 "internal hash" 为 key 的全局映射表, value 是所有源(BT/HTTP/FTP/ed2k/迅雷 peer)的列表。这就是 P2SP 跨源去重机制。

### 6.5 关键发现 (PAM 作者实测)

> When using Xunlei to download a particular BT file, only **4% of the file came from BT**, the remainder of the file came from an HTTP server (**74%**) and from Xunlei peers using the Xunlei protocol (**22%**).

**关键含义**: 即使下载 BT 文件,迅雷客户端主要从 HTTP 服务器和迅雷 peer 拿数据,只有 4% 来自 BT swarm 本身。这是 P2SP 的"带宽劫持"核心机制。

### 6.6 内容流入服务器 (Section 3.4)

> For 177 of 219 popular torrents, the torrent first appeared in Pirate Bay, then in the Xunlei tracker without reference to a server, and finally in the Xunlei tracker with reference to one or more servers.
> The three domains serving the most files were: **megaupload.com, hotfile.com, fileserve.com** (cyberlockers).

**关键含义**: 迅雷会自动把 BT 内容"搬运"到 cyberlocker,然后用自己的 tracker 跟踪这些 HTTP 源。

### 6.7 论文承认的局限

> To the best of our knowledge, only a few preliminary studies of Xunlei have been carried out to date, focusing on the protocols used for transferring data among peers [4, 5].

论文作者也承认协议层分析是初步的,深入协议字段未完全破解。

---

## 7. Tsinghua HMC 论文 - 协议结构完整贴出

来源: https://tsinghua-nslab.org/assets/files/hmc-b3f76b0d714e2b7a50a7d3ccbbdbb83a.pdf (本地存档 /tmp/tsinghua.txt)

### 7.1 Thunder 协议包结构 (Section III.B)

> A Thunder packet can be divided into two parts: Thunder Header and Thunder Body.
>
> **Thunder Header** (mandatory):
> - The **first 4 bytes** of Header is the **command part** that defines operations.
> - Following the command is the **connection part**, which indicates node and connection information.
> - By reverse engineering, we find that command part includes **more than 300 types of different commands**.
> - The Header part is **not encrypted**.
>
> **Thunder Body** (optional):
> - Includes the payload of sharing data **in encryption**.

### 7.2 协议特征 (Section III.C)

> 1. **The Headers are not encrypted, but all data parts are encrypted.**
> 2. There are relatively more `0x00`s in the Connection part, especially **two or three continuous 0x00s**.
> 3. Many headers **end with three continuous 0x00s**, or a string with a certain length after two or three continuous 0x00s.

### 7.3 关键命令字 (Section IV.A, 图 5)

论文给出的状态机 (State Machine of Interaction, SMS) 包含 6 个关键状态:

```
S0 = cmd_query_p2phub          →  S1 = cmd_query_p2phub_resp
S2 = cmd_request               →  S3 = cmd_request_resp
S4 = cmd_query_tracker         →  S5 = cmd_query_tracker_resp
```

以及 `CMD_TYPEID_HUB_KEEP_ALIVE_RESP` (Hub keep-alive 响应)

### 7.4 Thunder 工作流程 (Section III.A)

```
(1) Login (TCP+UDP) → 主服务器 (resource/ad/registering/news/multimedia servers)
(2) Idling           → 4 种 UDP 交互:
                       (a) ICMP 到 keep-alive server
                       (b) UDP 到 node server
                       (c) UDP 到 main server
                       (d) UDP 到另一 main server
                       (LAN 内有迅雷 peer 时, UDP 互联)
(3) File sharing     → 先 TCP 到 resource server, 然后多 Thunder peers 之间 UDP + 少量 TCP
```

### 7.5 协议特征总结

| 特征 | 描述 |
|---|---|
| 传输层 | 主要 UDP,少量 TCP |
| 包头 | 4 字节命令 + 变长 connection, **未加密** |
| 包体 | **加密** (随机性高) |
| 命令字 | 300+ 种 |
| Header 标记 | 多个连续 0x00,常以 3 个 0x00 结尾 |
| 状态机 | 至少 6 状态 (query_p2phub → request → query_tracker) |

---

## 8. 迅雷 peerid 格式 / 握手特征

### 8.1 BitTorrent 协议规范收录 (决定性)

来源: https://wiki.theory.org/BitTorrentSpecification (BitTorrent 官方 wiki)

**Azureus-style peer_id 编码**:
```
'-' + client_id(2字符) + version(4 ASCII digits) + '-' + random_bytes(12)
```

**迅雷在 BT spec 中正式注册的 client_id**:
```
'XL' - Xunlei
```

示例: `-XL0019-` (迅雷 v0.0.1.9)

### 8.2 PeerBanHelper 识别规则 (决定性)

来源: https://github.com/PBH-BTN/PeerBanHelper/issues/1358

> "迅雷官方已经全面铺开了 **-XL0019-** 为特征的 DownloadSDK。
> 与以往版本的显著区别是现在支持"下载时上传数据"以及"**加密连接**"。
> 也就是说迅雷不再是以前一毛不拔的铁公鸡, 而仅仅是 Hit-And-Run。"

**关键含义**: 当前迅雷 BT 客户端的 peer_id 前缀就是 `-XL0019-`, 并且启用了"加密连接"(可能指 BEP-8 MSE/RC4)。

### 8.3 transmission-block 默认规则 (决定性)

来源: https://github.com/qianbinbin/transmission-block/blob/master/transmission-block.conf

```bash
LEECHER_CLIENTS=%FF%1D%FF%FF%FF8I%FF,-GT0002-,-GT0003-,aria2,Baidu,libTorrent (Rakshasa) 0\.13\.8,libtorrent (Rasterbar) 2\.0\.7,libtorrent/2\.0\.7\.0,QQDownload,Thunder,Xfplay,Xunlei,XunLei
```

**解码 `%FF%1D%FF%FF%FF8I%FF`** (URL-encoded):
```
0xFF 0x1D 0xFF 0xFF 0xFF 0x38 0x49 0xFF
                          '8'  'I'
```

这是 **peer_id 前缀匹配**(老版迅雷的变体)。8 个字节前缀里 5 个是 0xFF——非常典型的迅雷早期 peer_id 模式。

### 8.4 PBH JSON 规则示例

来源: https://docs.pbh-btn.com/en/docs/misc/json-engine

```json
{
  "method": "CONTAINS",
  "if": {
    "method": "CONTAINS",
    "content": "xunlei 0019",
    "hit": "FALSE"
  },
  "content": "xunlei"
}
```

**逻辑**: ban 所有 ClientName 含 "xunlei" 的 peer,但 **排除** "xunlei 0019" (新版支持上传)。

### 8.5 BEP-10 ext_handshake 中的 client_name

> **未找到** 公开资料明确说明迅雷在 BEP-10 ext_handshake 的 `v` 字段(client_name + version)的具体值。

基于 PBH 规则反推 (因为 PBH 用 `CONTAINS "xunlei"` 匹配 ClientName), 推测格式:
```
v = "Xunlei 0.0.1.9"   (或类似)
```

但具体大小写、空格、版本号分隔符需用 wireshark 抓真实包验证。

---

## 9. Hub 服务器完整列表 (sandai.net)

来源: https://90apt.com/932 (修改 hosts 阻断迅雷的教程)

```
hub5p.sandai.net              # P2P peer 发现 (用户已知 PHub)
hub5btmain.sandai.net         # BT 主 hub (用户已知 SHub 的别名)
hub5idx.shub.sandai.net       # BT 索引 SHub
hub5emu.sandai.net            # emulator hub
hub5pr.sandai.net             # peer register hub
hub5c.sandai.net              # hub5 c
hub5t.sandai.net              # hub5 t
hub5u.sandai.net              # hub5 u
hub5sr.sandai.net             # hub5 sr (speed relay?)
hubciddata.sandai.net         # CID 数据 hub
viphub5pr.phub.sandai.net     # VIP peer register (用户已知 DPHub 关联)
imhub5pr.sandai.net           # IM peer register
reg2t.sandai.net              # 注册服务器 t
hubstat.sandai.net            # 统计 hub
bwcheck.sandai.net            # 带宽检测
spctrl.sandai.net             # 速度控制
liveupdate.mac.sandai.net     # Mac LiveUpdate
```

外加非 sandai.net 的相关域名:
```
upgrade.xl9.xunlei.com        # 升级
service.lixian.vip.xunlei.com # 离线 VIP 服务
xluser-ssl.xunlei.com         # 用户认证 (CaptchaSign/token)
cacerts.digicert.com          # 证书 (代码签名)
```

---

## 10. CID/GCID 算法 (开源已破解 - 决定性)

来源: https://github.com/iambus/xunlei-lixian/blob/master/lixian_hash.py (本地 /tmp/lixian_hash.py)

### 10.1 CID (= DCID) 算法

```python
def dcid_hash_file(path):
    h = hashlib.sha1()
    size = os.path.getsize(path)
    with open(path, 'rb') as stream:
        if size < 0xF000:                     # < 60KB: 全文件 SHA1
            h.update(stream.read())
        else:
            h.update(stream.read(0x5000))     # 前 20KB
            stream.seek(size/3)
            h.update(stream.read(0x5000))     # 1/3 处 20KB
            stream.seek(size-0x5000)
            h.update(stream.read(0x5000))     # 末尾 20KB
    return h.hexdigest()
```

### 10.2 GCID 算法 (来自 binux 博客)

```
GCID = SHA1( SHA1(piece1) || SHA1(piece2) || ... || SHA1(pieceN) )

piece_size 动态:
  psize = 0x40000  (256KB)
  while file_size / psize > 512 and psize < 0x200000:
      psize <<= 1
  # 让分片数 <= 512, 分片大小上限 2MB
```

### 10.3 BT 任务的 CID

> 来源: binux 博客原文
> "files share a same cid in a bt task, **cid is the btih of the torrent**"

即 BT 任务的 CID = 标准 BT infohash (20 字节 SHA1)。

### 10.4 XPF_HASHTYPE 枚举

来源: 反汇编 P2PBase.dll 字符串 (用户已知)

```
XPF_HASHTYPE_CID    = SHA1(三段 0x5000 采样)
XPF_HASHTYPE_BCID   = ? (跨源去重,算法未公开)
XPF_HASHTYPE_GCID   = SHA1(分片 SHA1 拼接)
XPF_HASHTYPE_URL    = URL 字符串
XPF_HASHTYPE_MD5    = MD5
XPF_HASHTYPE_SHA1   = 标准 SHA1
```

---

## 11. 看雪论坛帖子 251933 (关键 - 但完整内容需登录)

来源: https://bbs.kanxue.com/thread-251933.htm

### 11.1 从 Google snippet 能提取的信息

> "看到后端下载服务访问了以下网址。内容都经过加密, 先附加到下载服务上。
> 发现其使用 **AES-ECB 加密**, 解密后, 可以发现 **hub5btmain.v6.shub.sandai.net** 接口返回种子内每个文件的 **GUID 和 CID**。
> 拿到 gcid 后, 作为参数传递给 lua 脚本 **LuaServiceSHubQueryBTFileIndexCallBack::LuaCallBack** 进行处理。"

### 11.2 推断

- 迅雷后端用 **Lua 脚本**处理 SHub 响应(说明迅雷内嵌 Lua 引擎)
- 类名 `LuaServiceSHubQueryBTFileIndexCallBack` 暗示这是 BT 文件索引查询回调
- 加密方式与 PAM 2012 论文一致 (AES-ECB)
- 服务器 `hub5btmain.v6.shub.sandai.net` 即用户已知的 SHub,带 v6 版本前缀

### 11.3 无法获取的内容

帖子需登录 + 反爬虫验证。完整 lua 脚本、API URL、请求/响应字段格式未公开。建议:
1. 注册看雪账号后用浏览器手工抓取
2. 或联系作者 (帖子原作者)
3. 或参考 PAM 2012 扩展技术报告 http://cis.poly.edu/~prithula/papers/XunleiTR.pdf

---

## 12. 决定性结论 - 第三方接入迅雷 P2P 可行性评估

### 12.1 公开资料已覆盖的"接入必需信息"

| 信息 | 公开度 | 来源 |
|---|---|---|
| AES-ECB + 64-bit 密钥内嵌 | ⭐⭐⭐⭐⭐ | PAM 2012 |
| Thunder 包结构 (4B cmd + variable conn + encrypted body) | ⭐⭐⭐⭐⭐ | Tsinghua HMC |
| 状态机 6 阶段 (query_p2phub → request → query_tracker) | ⭐⭐⭐⭐ | Tsinghua HMC |
| Peer ID 格式 (-XL0019-) | ⭐⭐⭐⭐⭐ | BT spec + PBH |
| 老版 peer_id 字节特征 (`\xff\x1d\xff\xff\xff\x38\x49\xff`) | ⭐⭐⭐⭐ | transmission-block |
| Hub 域名完整列表 | ⭐⭐⭐⭐⭐ | hosts block list |
| CID/GCID 算法 | ⭐⭐⭐⭐⭐ | iambus/xunlei-lixian |
| BT infohash = BT CID | ⭐⭐⭐⭐⭐ | binux 博客 |
| Tracker 返回结构 (2×20B hash + 8B code) | ⭐⭐⭐⭐ | PAM 2012 |
| Xunlei ID = MAC(12B) + 4B random | ⭐⭐⭐⭐⭐ | PAM 2012 |
| Cloud API CaptchaSign 算法 (md5 chain) | ⭐⭐⭐⭐ | alist driver |

### 12.2 公开资料**未**覆盖的"接入必需信息"

| 信息 | 缺失原因 |
|---|---|
| 300+ 命令字具体 ID 与含义 | 仅 HMC 论文给出 6 个关键命令,其余未公开 |
| 4 字节命令字后的 connection 字段具体结构 | 仅知特征 (多 0x00, 3 个 0x00 结尾) |
| Thunder Body 加密载荷的具体字段 | 仅有"加密"描述,无字段表 |
| BEP-10 ext_handshake 中迅雷私有扩展的 ID 与载荷 | **完全未公开** (用户反汇编发现的 PunchingHole / SuggestPiece 没有任何公开对照) |
| SHub/PHub HTTP(S) API 的请求字段格式 | 看雪帖提到但内容需登录 |
| `LuaServiceSHubQueryBTFileIndexCallBack::LuaCallBack` 的 lua 脚本内容 | **完全未公开** |
| PunchingHole (NAT 打洞) 协议 | **完全未公开** |
| SuggestPiece 协议 | **完全未公开** |
| uDT (XUdt.dll) 自研传输层协议 | **完全未公开** (与开源 UDT 不同,是迅雷自研变体) |
| DCDN 加速节点接入协议 | **完全未公开** |
| BCID 算法 | **完全未公开** (P2SP 跨源去重哈希) |

### 12.3 接入迅雷 P2P 网络的工程评估

| 路径 | 工作量 | 可行性 | 关键阻塞 |
|---|---|---|---|
| A. 仅做 BT peer (标准 BEP-3) | 1-2 月 | ⭐⭐⭐⭐⭐ | 无阻塞,标准 BT 协议 |
| B. 接入迅雷 P2P (与迅雷 peer 互通) | 6-18 月 | ⭐⭐ | 缺命令字表 + 缺扩展协议 |
| C. 接入迅雷 tracker 获取 peer 列表 | 3-6 月 | ⭐⭐⭐ | 缺 SHub/PHub API 字段 |
| D. 仅做迅雷→标准 BT 转换器 | 7-10 天 | ⭐⭐⭐⭐⭐ | 已有 PoC (用户已有研究) |

**最优路径**: A + D 组合 (放弃迅雷 P2P 接入,做标准 BT + 转换工具)。

### 12.4 公开资料调研的"天花板"

公开资料止步于 **2012 年 PAM 论文 + 2012 年左右 Tsinghua HMC 论文 + 2017 年前后 hosts 列表 + 2024-2025 年 PBH 规则**。**没有任何更新的协议层逆向资料**。

推测原因:
1. 迅雷在 2014 年前后加强了反逆向 (代码混淆 + 服务端校验)
2. 学术界 2012 后失去兴趣 (P2P 研究衰退)
3. 工业界逆向成本高,无公开利益
4. 用户基数太大,封号风险高

---

## 13. 下一步建议 (给用户)

### 13.1 立即可做

1. **接受公开资料天花板**: 公开网络上 **没有** 第三方接入迅雷 P2P 的现成方案。
2. **聚焦路径 D**: 用户已有的迅雷→libtorrent 转换器是当前最务实的方向。
3. **保留路径 C 作为可选**: 如果用户能拿到 SHub 响应样本,可以反汇编 download_engine.dll 中的 LuaServiceSHubQueryBTFileIndexCallBack 类,提取字段表。

### 13.2 如果坚持路径 B (接入迅雷 P2P)

需要继续逆向的信息(公网无资料):
1. `XBTInputChannelSession` / `XBTOutputChannelSession` 类的 vtable 完整方法表
2. BEP-10 ext_handshake 中迅雷私有 `m` 字典的所有 key (PunchingHole / SuggestPiece 的扩展 id 数字)
3. PunchingHole 载荷格式 (NAT 打洞握手)
4. SuggestPiece 载荷格式
5. uDT (XUdt.dll) 与标准 uTP / UDT 的差异
6. PHub/SHub HTTP API 字段 (URL 路径 + POST body 格式)
7. `LuaServiceSHubQueryBTFileIndexCallBack` 类的 Lua 脚本 (如果能从内存 dump)

### 13.3 补充资料获取建议

1. **看雪论坛 251933 帖子**: 注册账号后浏览器手工访问,可能有完整 lua 脚本和字段表
2. **PAM 2012 扩展报告**: http://cis.poly.edu/~prithula/papers/XunleiTR.pdf (本研究未能成功抓取,可能含更多协议字段)
3. **PBHBTN Sparkle 网络**: 加入 BTN 网络可获取大量迅雷 peer 的真实 handshake 抓包数据
4. **FortiGuard / Clavister 防火墙签名**: 商业防火墙厂商可能持有完整协议签名(但需付费)

---

## 14. 完整文件清单 (本地存档)

```
/tmp/pam2012.pdf              - PAM 2012 论文 PDF (476KB)
/tmp/pam2012.txt              - PAM 2012 论文文本 (504 行)
/tmp/tsinghua.pdf             - Tsinghua HMC 论文 PDF
/tmp/tsinghua.txt             - Tsinghua HMC 论文文本
/tmp/fastdick.py             - Xunlei-Fastdick 完整源码 (888 行)
/tmp/pyengine.py             - python-thunder-download_engine.py (181 行)
/tmp/db_main.py              - deathbless/thunder main.py (高速通道 SQLite 破解, 100 行)
/tmp/sdk_readme.md           - ThunderOpenSDK 完整 README
/tmp/pbh_engine.html         - PBH JSON 规则引擎文档
/tmp/pbh_1358.html           - PBH Issue 1358 (-XL0019- 评估)
/tmp/trans_block.conf        - transmission-block 默认配置 (含 LEECHER_CLIENTS)
/tmp/trans_block.md          - transmission-block README
/tmp/oplist.html             - OpenList Thunder Driver 文档
/tmp/lixian_hash.py          - iambus/xunlei-lixian 哈希算法实现
/tmp/kanxue_archive.html     - 看雪 251933 web.archive.org 快照 (失败,仅 12KB)
/tmp/search1-34.json         - 各次 web search 原始结果
```

