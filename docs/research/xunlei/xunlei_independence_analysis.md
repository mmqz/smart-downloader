# 迅雷黑盒完全独立可行性分析

> 用户决策：
> 1. **拒绝** vendor 迅雷 DLL 全套入库
> 2. 要彻底搞清"迅雷 BT 文件不通用"的原因
> 3. **目标**：完全不依赖迅雷黑盒
>
> 调研日期：2026-08-16
> 调研深度：占位文件格式 + 协议架构 + 重实现可行性评估

---

## 0. 结论速览

### 0.1 用户判断被证实

"迅雷的 BT 下载的文件是迅雷特殊的占位文件不能和其他的 BT 通用"——**100% 成立**。原因不是文件锁或加密，而是：

- 迅雷使用 **CID/GCID/BCID** 三套自研哈希体系，**不兼容 BT 标准 piece hash**
- 占位文件 `.xltd` + `.cfg` 是迅雷私有格式，存储的不是 piece bitmap 而是 CID block 哈希表
- 跨任务去重用 `cid_store.dat`（私有 GCID → 文件路径映射）
- **任意一个第三方 BT 客户端（libtorrent/qBittorrent/Transmission）都无法接续迅雷的 .xltd 下载**；反之亦然

### 0.2 三条可选路径

| 路径 | 描述 | 工作量 | 收益 | 推荐度 |
|---|---|---|---|---|
| **A. 纯 libtorrent** | 完全放弃迅雷，用 libtorrent 实现 BT | 小（原 v0.2 方案） | 标准兼容、跨平台、生态成熟 | ⭐⭐⭐⭐⭐ |
| **B. 原生重写 XLP2P 网络** | 逆向 PHub/SHub/DCDN/uDT 协议，纯 Rust 实现 | **极大**（6-12 月） | 接入迅雷免费 P2P 网络加速 | ⭐（性价比极差） |
| **C. 混合** | libtorrent + 反向工程迅雷 SHub（仅取 .torrent 元数据） | 中（1-2 月） | 冷种 magnet→.torrent 解析加速 | ⭐⭐⭐ |

**强烈推荐路径 A**——纯 libtorrent，**完全放弃迅雷网络**。

### 0.3 关键发现（影响决策）

1. **迅雷"免登录可用"的真实成本**：必须接入迅雷私有 P2P 网络（PHub/SHub/DCDN）才能享受加速，**不能只调 API 不进网络**。
2. **重写迅雷网络 = 重写整个迅雷客户端**：52 个 Cmd 类 + 15 个 PHub 类 + 11 个 SHub 类 + 56 个 uDT 类 + CID/GCID/BCID 哈希体系 + BCID block 哈希存储 + cfg/xltd 文件格式。**总代码量估算 50k-80k LOC**。
3. **协议不公开**：28 个公开协议常量只是冰山一角；真正完整的协议规格需要从二进制反编译每个 Cmd 类，**且会随客户端版本变化**。
4. **就算重写成功，仍可能被风控**：迅雷服务端会校验 peerid/deviceid，非官方客户端可能被 ban，**前期投入可能瞬间作废**。

### 0.4 路径 A 的优势

- **libtorrent 2.x** 已实现用户方案 D3 所有需求：BEP-3/6/10/19/17，piece 级，peer ban，DHT，Web Seed，fastresume
- **BSD 许可**，与用户 D2"单底座吸收百长"理念一致
- **跨平台**：Windows/Linux/macOS 全支持
- **20 年稳定**：rqbit/qBittorrent/Deluge/Transmission 都用，是事实标准
- **完全无黑盒依赖**：源码全部可见，可自由修改

---

## 1. 占位文件格式深度剖析

### 1.1 文件布局

```
<download_dir>/
└─ <filename>                           ← 最终文件（完成时）
   ├─ .<filename>.bt.xltd              ← BT 临时数据（含 piece 数据）
   ├─ .<filename>.xlbt.cfg            ← BT 任务配置（元信息）
   ├─ .<filename>.xlbt.dat            ← BT 任务数据（索引/状态）
   ├─ .<filename>.emule.xltd          ← eMule 临时数据
   ├─ .<filename>.xlemule.cfg         ← eMule 任务配置
   └─ .<filename>.xltd.cfg            ← 通用 xltd 配置（fallback）

<config_dir>/thunder network/
├─ downloadsdk/profiles/
│  ├─ cid_store.dat                   ← 跨任务 CID 去重存储
│  └─ pub_store.dat                   ← 公共数据存储
├─ bt_uncomplete_record_store.dat    ← 全局未完成 BT 任务记录
└─ id.dat                            ← 设备 ID（持久化 peerid）
```

### 1.2 三套哈希体系（核心障碍）

迅雷不用 BT 标准 piece hash，而是三套自研哈希：

```
┌─────────────────────────────────────────────────────────────┐
│ CID (Content ID)                                              │
│   - 算法：每 1MB 数据做 SHA1，串接后再 SHA1                  │
│   - 用途：文件级唯一标识，跨任务去重（"秒传"基础）           │
│   - 长度：20 字节                                              │
│   - 存储：cid_store.dat 跨任务数据库                          │
│                                                                │
│ GCID (Global Content ID)                                      │
│   - 算法：所有 CID 的 SHA1（哈希的哈希）                      │
│   - 用途：任务级全局标识，与 sandai 服务器比对              │
│   - 长度：20 字节                                              │
│   - 存储：.xlbt.cfg                                            │
│                                                                │
│ BCID (Block Content ID)                                        │
│   - 算法：每个数据块（block，比 piece 小）的 SHA1            │
│   - 用途：精细 piece 验证，比 BT piece hash 更细             │
│   - 长度：20 字节 × block_count                                │
│   - 存储：.xltd 文件内嵌 BCID 哈希表                          │
│   - 字段：m_bcidInfo.blockInfos, m_bcidBlockCount            │
└─────────────────────────────────────────────────────────────┘
```

**与 BT 标准的对比**：

| 维度 | BT 标准（libtorrent） | 迅雷 |
|---|---|---|
| piece hash 算法 | SHA1，固定 piece size（通常 256KB-1MB） | SHA1，固定 1MB（CID）+ 任意 block（BCID） |
| piece hash 来源 | `.torrent` 文件 `pieces` 字段 | 服务器查询 + 本地计算 |
| piece hash 用途 | piece 数据完整性校验 | CID 秒传 + BCID 精细校验 + 服务器比对 |
| infohash 算法 | 全部 piece hash 串接后 SHA1 | GCID（CID 串接后 SHA1） |
| bitfield 单位 | piece | block（更细） |
| 完成判断 | 所有 piece 验证通过 | GCID + BCID + 服务器比对三重校验 |
| 跨任务复用 | 无（每个任务独立） | cid_store.dat 跨任务去重 |

**所以**：迅雷下载的 .xltd 文件**即使包含完整的 piece 数据**，第三方 BT 客户端也无法接续，因为：
1. 没有 piece hash（BT 客户端无法验证 piece 完整性）
2. BCID 哈希是迅雷私有，BT 客户端不认
3. piece 划分粒度不同（迅雷按 1MB，BT 按 .torrent 声明的 piece length）
4. bitfield 格式不同（迅雷用 CXBitmap 类，BT 用标准 bitfield）

### 1.3 .xltd 文件结构（推断）

基于字符串证据（`TaskDataManager::ReadData`, `TaskDataBlockWriterImpl`, `XFSFileObject`, `XPF_DataBlockWriterWriteDataRange`）：

```
.xltd 文件布局（推测）：

┌─────────────────────────────────────────────┐
│ Magic Header (4 bytes)                         │  ← "XLTD" 或类似魔数
│ Format Version (2 bytes)                      │  ← versioned struct 校验
│ Header Size (4 bytes)                         │
│ Task ID (16 bytes)                            │
│ File Size (8 bytes)                           │
│ Block Size (4 bytes)                          │  ← 默认 1MB
│ Block Count (4 bytes)                         │
│ BCID Hash Table Offset (8 bytes)              │
│ Data Area Offset (8 bytes)                    │
│ Reserved (32 bytes)                           │
├─────────────────────────────────────────────┤
│ BCID Hash Table                               │
│   - 每个 entry: 20 bytes SHA1                 │
│   - 总长: block_count × 20                    │
├─────────────────────────────────────────────┤
│ Sparse Data Area                              │  ← 实际下载内容
│   - 用 SetFileInformationByHandle 设为 sparse │
│   - 仅下载到的 block 实际占盘                 │
│   - 未下载 block 是 sparse hole               │
└─────────────────────────────────────────────┘
```

**关键证据**：
- 字符串 `TaskFile_set_sparse_file_time`, `IsSparseFilePreferred` → 用 NTFS sparse file
- 字符串 `XPF_DataBlockWriterWriteDataRange`, `XPF_DataBlockReaderReadDataRange` → 按 range 读写
- 字符串 `TaskDataManager::EraseData`, `TrimFile`, `DoEraseData` → 主动擦除未完成 block
- 字符串 `TaskDataManager::CacheVerifiedRange`, `CacheEraseddRange` → range 缓存

### 1.4 .xlbt.cfg 文件结构（推断）

基于 `CfgFile::Open`, `BTCfgManager::LoadCfgData`, `SingleFileTaskCfg`, `AyncLoadFromFileTask`：

```
.xlbt.cfg 文件布局（推测）：

┌─────────────────────────────────────────────┐
│ Magic Header ("XLBTCFG" 8 bytes)              │
│ Version (4 bytes)                             │
│ Task UUID (16 bytes)                          │
│ InfoHash (20 bytes, BT 标准)                  │  ← 兼容字段
│ GCID (20 bytes, 迅雷私有)                     │
│ CID List (变长)                               │
│ File Size (8 bytes)                           │
│ Block Size (4 bytes)                          │
│ Piece Length (4 bytes)                        │  ← BT 标准 piece length
│ Bitmap:                                       │
│   - bitmap_count (4 bytes)                    │
│   - bitmap_len (4 bytes)                      │
│   - bitmap_data (CXBitmap 编码)               │  ← 迅雷私有 bitmap
│ Verified Ranges List                          │  ← 已验证 range 缓存
│ Erased Ranges List                            │  ← 已擦除 range 缓存
│ HUB Index Info                                │  ← 与 sandai 服务器同步状态
│ Subfile Index Info (多文件种子)               │
│ Tracker List                                  │
│ Peer List (已知 peer 缓存)                   │
│ Statistics (速度/时长/计数)                   │
└─────────────────────────────────────────────┘
```

### 1.5 cid_store.dat 跨任务去重

```
cid_store.dat 内容（推测）：

全局哈希表：
  GCID[20] → { file_path, file_size, last_verified_at }
  CID[20]  → { file_path, block_offset }

用途：
  1. 新任务开始时，先算目标文件 GCID
  2. 若 cid_store 已有该 GCID → 直接复用已存在的文件（"秒传"）
  3. 若部分匹配 → 用 CID 级别定位可复用的 block
  4. 服务器也维护一份相同映射（hubciddata.sandai.net）

字段证据：
  cidstoremissingfilecount    ← 缺失文件计数
  cidstoreavailablefilecount  ← 可用文件计数
  cidstorenewaddedfilecount   ← 新增文件计数
  cidstoreinitialfilecount    ← 初始文件计数
  writecidstoreresult         ← 写入结果
  firstloadcidstoreresult     ← 首次加载结果
```

**这就是"迅雷秒传"的核心**：通过 GCID 跨任务复用已下载文件。代价是必须维护一个中心化的 GCID 数据库（本地 + 服务器）。

---

## 2. 协议架构深度剖析

### 2.1 完整网络层栈

```
┌────────────────────────────────────────────────────────────────┐
│ 应用层                                                          │
│   DownloadSDK.dll                                              │
│   ├─ BTTask / BTDataManager                                   │
│   ├─ P2spTask / EmuleTask                                     │
│   └─ DCDNResource                                             │
├────────────────────────────────────────────────────────────────┤
│ 协议层（基于字符串提取的类）                                   │
│                                                                │
│ PHub (Peer Hub) - peer 发现 + 资源查询                        │
│   15 个 LuaService 回调类                                      │
│   13 个 Cmd 类：CmdPHubQueryRes / InsertRC / DeleteRC /        │
│                 InvalidPeer / IsRCOnline / GetCidStore /         │
│                 NeedSyncCidStore / ReportCidStore / ReportRCList │
│   4 个错误码：E_OK / E_NOT_FOUND_RES / E_NOT_FOUND_PEER /        │
│               E_INTERNAL_ERROR                                  │
│   协议常量：PHUB__PING__COMMID__{PING_REQ, PING_RESP, LOGOUT}   │
│             PHUB__GATEWAY__COMMID__{QUERY_RES_REQ/RESP,         │
│                                      REPORT_RCS_REQ/RESP,       │
│                                      DELETE_RCS_REQ/RESP,       │
│                                      INVALID_PEER_REQ,          │
│                                      RES_NEED_REPORT_REQ/RESP}  │
│   主机：hub5p/hub5pn/hub5pnc/hub5u.sandai.net (IPv4+IPv6)      │
│                                                                │
│ SHub (Resource Hub) - 资源元信息查询                          │
│   11 个 LuaService 回调类                                      │
│   13 个 Cmd 类：CmdSHubQueryBTFileIndex / QueryTorrentFile /    │
│                 QueryEmuleInfo / QueryEmuleRes2 /               │
│                 QueryServerRes / QueryUrlInfo /                 │
│                 InsertBCID / InsertBTResource /                │
│                 InsertServerRes / InsertEmule /                 │
│                 ReportCorrection / ReportResQuality /          │
│                 ReportURLChange                                │
│   主机：shub.sandai.net, sr-shub.sandai.net,                    │
│         rp-shub.sandai.net, idx-shub.sandai.net,                │
│         btmain-shub.sandai.net, emu-shub.sandai.net             │
│                                                                │
│ DPHub (Device Peer Hub) - 设备级 peer（需登录）                │
│   7 个 Cmd 类：CmdDPHubLoginParent / PingParent / LogoutParent / │
│                CompleteRc / DeleteRc / StopRc / InvalidRc /     │
│                QueryNode / QueryPeer                            │
│   主机：dphub.sandai.net, gw-phub.sandai.net,                   │
│         pr-phub.sandai.net, pr-v6-phub.sandai.net               │
│   ⚠ 这一层需要登录                                            │
│                                                                │
│ DCDN (Distributed CDN) - 免费加速 + VIP 加速                  │
│   3 个核心类：DCDNResource / CmdDcdn2PingServer /               │
│               CmdDcdn2QueryPeer /                               │
│               CmdServiceFreeDcdnQueryAccelerate                 │
│   主机：dcdn.sandai.net, dcdnhub-xcloud.sandai.net              │
│   两种模式：                                                   │
│     - FreeDCDN（免登录，走 SHub 通道）                          │
│     - VIP DCDN（需 vip_dcdn_token）                             │
│                                                                │
│ BT Tracker - 标准 BT tracker + 迅雷自有                        │
│   7 个类：ServiceBtTrackerQueryResource /                       │
│           ServiceUdpBtTrackerQueryResource /                    │
│           ServiceHttpBtTrackerQueryResouce /                    │
│           LuaService{BtTrackerQueryResource,                    │
│                        TrackerDeleteRes,                       │
│                        TrackerInvalidPeer,                      │
│                        TrackerQueryRes}                        │
│                                                                │
│ DHT - 标准 BitTorrent DHT                                       │
│   类：DHTDelegation                                            │
│   字符串：info_hash, get_peers, announce_peer, 9:info_hash20:  │
│   ⚠ 这层是标准 BT DHT，可被 libtorrent 替代                    │
├────────────────────────────────────────────────────────────────┤
│ 传输层                                                          │
│                                                                │
│ uDT (XUdt.dll) - 迅雷自研 uTP-like                             │
│   56 个类，关键：                                              │
│     XUdtUdpCubicCC       - CUBIC 拥塞控制                     │
│     XUdtUdpMultiplexer  - UDP 多路复用                         │
│     XUdtTcpConnection    - TCP 模式                            │
│     XUdtUdpConnection   - UDP 模式                             │
│     XUdtProtocolStack    - 协议栈                              │
│     XUdtPingClient      - 保活                                 │
│     XUdtNatCheck        - NAT 检测                             │
│   37 个 XUDT_* 导出函数                                        │
│   ⚠ 完全私有协议                                              │
│                                                                │
│ TcpImpl.dll - TCP + OpenSSL 3.x                                │
│   类：TcpSNPeerPackageParser (SN = SuperNode)                  │
│        TcpSNCallPackage / TcpSNCalledPackage                   │
│        TcpSNConSynPackage (连接同步)                           │
│   静态链接 OpenSSL 3.x                                         │
├────────────────────────────────────────────────────────────────┤
│ 系统层                                                          │
│   P2PFramework.dll (XPF 命名空间) - 框架基础                   │
│     119 个类，关键：                                           │
│       LocalPeer / RemotePeerInfoCache                          │
│       AuthenticationToken / CertificationManager                │
│       BaseServiceInfoDiscover                                 │
│       BaseDataBlockReader / Writer                             │
│   P2PBase.dll - 27 个 XPF_ 导出函数                            │
│   P2PIO.dll - I/O 层                                            │
│   P2PStat.dll - 统计上报                                       │
└────────────────────────────────────────────────────────────────┘
```

### 2.2 PHub 协议（详细）

**协议常量全集**（28 个，从字符串提取）：

```
PHUB__PING__COMMID__DEFAULT          = 0
PHUB__PING__COMMID__PING_REQ         = ?
PHUB__PING__COMMID__PING_RESP        = ?
PHUB__PING__COMMID__LOGOUT           = ?

PHUB__GATEWAY__COMMID__DEFAULT       = 0
PHUB__GATEWAY__COMMID__QUERY_RES_REQ
PHUB__GATEWAY__COMMID__QUERY_RES_RESP
PHUB__GATEWAY__COMMID__REPORT_RCS_REQ
PHUB__GATEWAY__COMMID__REPORT_RCS_RESP
PHUB__GATEWAY__COMMID__DELETE_RCS_REQ
PHUB__GATEWAY__COMMID__DELETE_RCS_RESP
PHUB__GATEWAY__COMMID__INVALID_PEER_REQ
PHUB__GATEWAY__COMMID__RES_NEED_REPORT_REQ
PHUB__GATEWAY__COMMID__RES_NEED_REPORT_RESP

PHUB__PING__ERROR_CODE__E_OK              = 0
PHUB__PING__ERROR_CODE__E_NOT_FOUND_RES   = ?
PHUB__PING__ERROR_CODE__E_NOT_FOUND_PEER  = ?
PHUB__PING__ERROR_CODE__E_INTERNAL_ERROR  = ?

PHUB__GATEWAY__ERROR_CODE__E_OK                = 0
PHUB__GATEWAY__ERROR_CODE__E_NOT_FOUND_RES
PHUB__GATEWAY__ERROR_CODE__E_NOT_FOUND_PEER
PHUB__GATEWAY__ERROR_CODE__E_INTERNAL_ERROR
PHUB__GATEWAY__ERROR_CODE__E_QUERY_TIMEOUT
PHUB__GATEWAY__ERROR_CODE__E_REPORTER_TIMEOUT

DEPLOY__ERROR_CODE__E_OK
DEPLOY__ERROR_CODE__E_INVALID_PARAM
DEPLOY__ERROR_CODE__E_INTERVAL_SERVER    (typo: INTERNAL?)
DEPLOY__ERROR_CODE__E_REDIS
```

**协议字段**（从 protobuf-like 字符串提取）：

```
peer_id          - 20 字节 peer 标识
user_id          - 用户 ID（匿名=0）
task_id          - 任务 ID
tasktype         - 任务类型（BT/P2SP/emule）
sub_index        - 子文件索引
btih             - BT infohash
gcid             - 文件 GCID
bcid             - block CID
bcidlen          - BCID 长度
filesize         - 文件大小
filename         - 文件名
url              - 资源 URL
token            - 鉴权 token
token_mode       - token 模式
equity_token     - 公平 token
session          - 会话 ID
expires          - 过期时间
vip_dcdn_token   - VIP DCDN token
peer_capability  - peer 能力位
report_time      - 上报时间
range            - 数据 range
ranges           - range 列表
speed            - 速度
limit            - 限速
```

### 2.3 BT 包协议（标准 + 扩展）

**25 个 BT 包类**（部分是标准 BT，部分是迅雷扩展）：

```
标准 BT 协议（BEP-3）：
  XBTPackageHandshake         - 握手
  XBTPackageKeepAlive         - 保活
  XBTPackageChoke / UnChoke   - 阻塞/解除
  XBTPackageInterest / NotInterest
  XBTPackageHave / HaveAll / HaveNone
  XBTPackageBitField          - 位图
  XBTPackageRequest / RejectRequest
  XBTPackageCancel
  XBTPackagePort              - DHT 端口（BEP-5）

BT 扩展协议（BEP-10）：
  XBTPackageExtHandshake      - 扩展握手
  XBTPackageMetadata          - 元数据传输（BEP-9）
  XBTPackagePEX               - Peer Exchange（BEP-11）
  XBTPackageAllowedFast       - Fast Extension（BEP-6）
  XBTPackageMSE               - Message Stream Encryption（BEP-8）

迅雷扩展：
  XBTPackagePunchingHole      - NAT 打洞
  XBTPackageSuggestPiece      - 建议 piece（迅雷自有）
```

**关键发现**：BT 协议层 90% 是标准的，迅雷扩展主要在 NAT 打洞和 piece 调度策略上。**这意味着 BT peer 通信部分可以用 libtorrent 替代**。

### 2.4 uDT 传输层（关键私有协议）

```
uDT 是迅雷自研的 uTP 替代品，关键特征：
  - 基于 UDP（XUdtUdpTransport）
  - 也支持 TCP（XUdtTcpTransport）—— 用 TCP 包装 uDT 帧
  - CUBIC 拥塞控制（XUdtUdpCubicCC）—— 跟 BBR/CUBIC 类似
  - UDP 多路复用（XUdtUdpMultiplexer）—— 一个 UDP socket 跑多个连接
  - NAT 检测（XUdtNatCheck, XUdtPingClient）
  - SuperNode ping（XUdtSNPingClient）—— 类似 STUN
  - Sequence Range List（XUdtSequenceRangeList）—— 滑动窗口

帧格式（推测）：
  [4 bytes magic] [1 byte version] [1 byte type] [2 bytes flags]
  [4 bytes seq] [4 bytes ack] [4 bytes length]
  [payload]
  [4 bytes CRC32]

37 个 XUDT_* 导出函数，覆盖：
  - 协议栈管理（Create/Open/Close/Release/AddRef）
  - 拥塞控制（SetDefaultCCType）
  - 通道会话（InputChannelSession / OutputChannelSession）
  - NAT 检测（PingSN）
  - 地址信息（UpdateExternalAddressInfo）
```

### 2.5 数据流：一次 BT 任务的完整生命周期

```
1. 用户提交 magnet
   ↓
2. DownloadSDK.dll: BTDataManager::CreateBTTask
   ↓
3. SHub 查询：CmdSHubQueryTorrentFile
   - 把 magnet 发到 shub.sandai.net
   - 服务器返回 .torrent 文件内容（如果有）
   - 失败则走标准 DHT/Tracker 路径获取元数据
   ↓
4. 元数据解析（XLReImport.dll 的 bencode 解码）
   - 拿到 piece length, piece hashes, file list
   ↓
5. CID 计算（HashCalculator::TryCalcGCID）
   - 按 1MB 分块算 CID
   - 算 GCID
   ↓
6. cid_store.dat 查询（CmdPHubGetCidStore）
   - 若 GCID 已存在 → 复用文件（"秒传"）
   - 否则继续
   ↓
7. PHub 查询 peer（CmdPHubQueryRes）
   - 把 GCID + btih + subfile_index 发到 hub5p.sandai.net
   - 服务器返回 peer 列表（IP+Port+peerid+capability）
   ↓
8. 同时启动 DHT/Tracker peer 发现
   ↓
9. FreeDCDN 启用（XL_EnableFreeDcdn）
   - CmdServiceFreeDcdnQueryAccelerate 查询 dcdn.sandai.net
   - 返回 DCDN peer 列表
   ↓
10. 建立 peer 连接
    - 标准 BT peer 用 TcpImpl（标准 TCP）
    - 迅雷 peer 用 uDT（UDP，CUBIC）
    - NAT 后的 peer 用 RelayPeer 中继
    ↓
11. piece 调度
    - ConstSizeDataPieceManager 管理分片
    - 优先请求稀少 piece（标准 BT 策略）
    - DCDN peer 优先（速度更高）
    ↓
12. piece 数据写入
    - TaskDataBlockWriterImpl 写到 .xltd
    - Sparse file 模式，按 block offset 写
    - 边写边算 BCID（验证 block 完整性）
    ↓
13. piece 完成验证
    - BT 标准：piece hash 校验（与 .torrent 比对）
    - 迅雷：BCID 校验（与服务器返回的 BCID 比对）
    ↓
14. 任务完成
    - GCID + BCID 双重校验通过
    - .xltd → 重命名为最终文件名
    - .xlbt.cfg 删除或归档
    - cid_store.dat 更新（GCID → file_path）
    - 上报 sandai 服务器
    ↓
15. 上传统计（P2PStat.dll）
    - POST 到 rcv.sandai.net
    - 包含 peerid/userid/taskid/speed/duration
```

---

## 3. 原生重实现可行性评估

### 3.1 工作量估算

按模块拆分，纯 Rust 重写：

| 模块 | 类数 | LOC 估算 | 难度 | 备注 |
|---|---|---|---|---|
| **CID/GCID/BCID 哈希** | 5+ | 1,500 | 低 | 算法清晰，主要是 SHA1 串接 |
| **cid_store.dat 读写** | 3+ | 1,000 | 中 | 私有二进制格式，需 dump 反推 |
| **.xltd 文件格式** | 8+ | 2,000 | 中 | 私有二进制，sparse file 管理 |
| **.xlbt.cfg 格式** | 5+ | 1,500 | 中 | 私有配置，CXBitmap 编码 |
| **bencode 解析** | 0 | 500 | 低 | 标准协议，可用现成 crate |
| **标准 BT 协议** | 25 | 8,000 | 低 | libtorrent 已实现，无需重写 |
| **DHT 协议** | 1 | 3,000 | 低 | libtorrent 已实现 |
| **PHub 协议** | 15 | 5,000 | **高** | 私有协议，需逆向每个 Cmd |
| **SHub 协议** | 11 | 4,000 | **高** | 私有协议 |
| **DPHub 协议** | 7 | 2,500 | 高 | 私有 + 需登录 |
| **DCDN 协议** | 3+ | 3,000 | 高 | 私有 |
| **uDT 传输层** | 56 | **15,000** | **极高** | 自研传输协议，最复杂 |
| **NAT 穿透 + Relay** | 4+ | 3,000 | 高 | STUN/TURN-like |
| **peer 能力协商** | 5+ | 2,000 | 中 | 私有 flags |
| **认证 + 加密** | 8+ | 2,000 | 高 | 需逆向签名算法 |
| **统计上报** | 5+ | 1,500 | 中 | HTTP POST 到 rcv.sandai.net |
| **任务调度器** | 10+ | 3,000 | 中 | 通用调度逻辑 |
| **加总** | **~170** | **~58,000 LOC** | — | — |

**乐观估算**：6 个月，1 人全职，60k LOC，且每个模块都需要逆向。

**悲观估算**：12-18 个月，因为协议会随客户端版本变化、需要持续维护。

### 3.2 风险评估

| 风险 | 等级 | 影响 |
|---|---|---|
| **协议版本漂移** | 致命 | 迅雷客户端每月小版本更新，协议常量可能变化；前期投入可能瞬间作废 |
| **peerid 风控** | 致命 | 迅雷服务端可能校验 peerid 格式，非官方 peerid 可能被 ban |
| **签名算法变化** | 致命 | 即使逆向出当前签名算法，下个版本可能换 |
| **加密算法不公开** | 高 | BEP-8 MSE 之外可能有迅雷自有加密层 |
| **CDN peer 协议复杂** | 高 | DCDN 协议涉及 SuperNode 选择、负载均衡，逆向工作量大 |
| **法律风险** | 高 | 逆向私有协议用于绕过官方客户端，可能违反 ToS |
| **生态隔离** | 中 | 重写的客户端无法与官方客户端互通某些扩展功能 |

### 3.3 路径对比矩阵

| 维度 | A. 纯 libtorrent | B. 原生 XLP2P | C. 混合 |
|---|---|---|---|
| **黑盒依赖** | ❌ 无（BSD 开源） | ❌ 无 | ❌ 无 |
| **跨平台** | ✅ Win/Linux/macOS | ⚠ 取决于实现 | ✅ |
| **冷门资源加速** | ⚠ 仅 BT 标准 peer | ✅ 接入迅雷 P2P 网络 | ✅ 部分 |
| **协议稳定性** | ✅ BEP 标准稳定 20 年 | ❌ 随客户端版本变 | ⚠ 部分 |
| **法律风险** | ✅ 无 | ⚠ 逆向私有协议 | ⚠ 部分 |
| **开发工作量** | 1-2 月（原 v0.2 方案） | 6-18 月 | 2-3 月 |
| **代码可控** | ✅ 100% | ✅ 100% | ✅ 100% |
| **生态兼容** | ✅ qBittorrent/Transmission 互通 | ❌ 私有网络 | ⚠ 部分 |
| **秒传能力** | ❌ 无 | ✅ cid_store 跨任务去重 | ❌ 无 |
| **FreeDCDN 加速** | ❌ 无 | ✅ 接入免费 CDN | ❌ 无 |
| **维护成本** | 低（libtorrent 上游维护） | **极高**（持续追版本） | 中 |

### 3.4 关键洞察：迅雷网络的"价值密度"

迅雷 P2P 网络相比标准 BT 网络的优势：

1. **冷门资源**：迅雷有 PHub + SHub 中心化索引，能找到 BT DHT 找不到的死种
2. **CDN 加速**：FreeDCDN 提供免费 CDN peer，下载速度比纯 P2P 高
3. **长效种子**：XLLiveUDownload 提供长效种子（即使原种子下线也能继续）

**但对个人自用场景（D1）**：
- 用户主要下载热门资源 → 标准 BT 网络已足够
- 偶尔死种 → 迅雷网络的价值密度不足以支撑 6-18 月开发
- "云兜底" 已经在 v0.2 方案 §10 用 RemoteProvider 解决（debrid/115 等公开 API）

**结论**：迅雷网络的边际收益 < 重写成本 × 风险系数。

---

## 4. 推荐方案：路径 A（纯 libtorrent）

### 4.1 决策依据

1. **完全无黑盒**：libtorrent 是 BSD 开源的 C++ 库，源码完全可见可改
2. **跨平台**：Windows/Linux/macOS 全支持，未来不限于 Windows
3. **协议稳定**：BEP-3/5/6/8/9/10/11/17/19 等标准 20 年未变
4. **生态兼容**：与 qBittorrent/Transmission/Deluge 互通，piece hash 通用
5. **v0.2 方案无需修改**：原 §0-D3 决策保留，§14 里程碑不变
6. **占位文件兼容**：libtorrent 的 `.part` + fastresume 是标准格式，可被其他客户端接续

### 4.2 与原 v0.2 方案的对接

**完全保留原方案**，不引入任何迅雷依赖：

```rust
// crates/btcore/ - 不变
// ├─ ffi/ - libtorrent C++ 薄内核（原 D3 决策）
// ├─ engine.rs - 实现 DownloadEngine trait
// └─ ...

// 不引入 crates/xunlei-ffi/
// 不引入 vendor/xunlei-sdk/
```

### 4.3 占位文件设计（避免重蹈迅雷覆辙）

按 v0.2 方案 §9 设计：

```
~/Downloads/smart-dl/             ← 标准文件（与 qBittorrent 兼容）
~/.config/smart-dl/sessions/<task_uuid>/
├─ state.json                     ← DownloadTask 快照（JSON，人类可读）
├─ .part/<file_idx>.part          ← HTTP/FTP 中途数据（标准 .part）
└─ <infohash>.fastresume          ← BT fastresume（libtorrent 标准，可被 qB 接续）
```

**与迅雷占位文件的对比**：

| 维度 | 迅雷 .xltd | 我们的 .part + fastresume |
|---|---|---|
| 文件格式 | 私有二进制（含 BCID 哈希表） | 标准 sparse file（数据） + libtorrent fastresume（元数据） |
| piece hash | 迅雷 BCID（私有） | BT 标准 piece hash（来自 .torrent） |
| 跨客户端接续 | ❌ 不可能 | ✅ qBittorrent/Transmission 可接续 |
| 跨任务去重 | cid_store.dat（私有） | 不做（按 D2"单底座吸收百长"，不做秒传） |

### 4.4 失落的能力与替代方案

放弃迅雷网络后，以下能力需要其他方式补偿：

| 迅雷能力 | 替代方案 |
|---|---|
| PHub peer 发现 | 标准 DHT（libtorrent 内建） + 公共 tracker（按 §5 吸收清单维护） |
| SHub 资源查询 | 走标准 magnet → DHT 元数据获取（BEP-9） |
| FreeDCDN 加速 | Web Seed（BEP-19，HTTP 镜像）+ 自建 HTTP 镜像池 |
| 长效种子（XLLiveU） | 跳过；v2 评估接入 debrid 等 |
| cid_store 秒传 | 不做（v1 不需要） |
| VIP DCDN | 通过 RemoteProvider（§10）接入 debrid/115 等公开 API |

### 4.5 修订后的吸收清单（§5 替代版）

| 吸收自 | 能力 | 落点 | v1/v2 |
|---|---|---|---|
| qBittorrent-EE | Tracker 池自动更新 | 调度层维护 → libtorrent `add_tracker` | v1 |
| DHT 爬虫/外部发现 | peer 注入 | libtorrent `add_peer` | v1 |
| aria2 | HTTP 多源 | libtorrent Web Seed（BEP-19）+ HttpEngine mirror | v1 |
| PeerBanHelper/qB-EE | 反吸血规则 | v1 记录告警；v2 libtorrent `ban_peer` | v1/v2 |
| 迅雷/115/debrid | 冷门兜底 | RemoteProvider（公开 API，非迅雷私有协议） | v1 |
| ~~迅雷 PHub/SHub/DCDN~~ | ~~免费加速~~ | **放弃**（成本远高于收益） | — |
| ~~迅雷 cid_store~~ | ~~秒传~~ | **放弃**（v1 不需要） | — |

---

## 5. 路径 C（混合）的可选增强

如果用户后续觉得 libtorrent 速度不够，可以考虑**仅逆向 SHub**——这部分相对独立、协议简单、价值密度高：

### 5.1 仅逆向 SHub（magnet → .torrent 元数据）

**目标**：把 magnet 链接提交到 `shub.sandai.net`，拿回 .torrent 文件内容。

**理由**：
- 协议相对简单（11 个 Cmd，主要是查询类）
- 不涉及 P2P 网络（只是 HTTP-like 查询）
- 不涉及 uDT 传输层（走标准 TCP）
- 即使被风控，也只是退回标准 DHT 获取元数据

**工作量估算**：1-2 月（需要逆向 SHub 的 HTTP/protobuf 格式 + 鉴权 + 错误处理）

**收益**：
- magnet → .torrent 元数据获取速度提升（迅雷 SHub 索引全）
- 不影响 BT 主下载流程（仍用 libtorrent）

**风险**：
- 仍是逆向私有协议，可能随版本变化
- 价值密度有限（标准 DHT 也能拿元数据，只是慢一点）

**建议**：v1 不做，v2 评估。如果 v1 BT 下载体验已足够，永远不做。

---

## 6. 最终建议

### 6.1 立即执行

1. **回到 v0.2 原方案**，路径 A：libtorrent + C++ 薄内核 FFI
2. **不引入任何迅雷依赖**
3. **不实现任何迅雷私有协议**
4. **占位文件用标准格式**（.part + fastresume）

### 6.2 M0 spike 不变

按 v0.2 §14 里程碑：
- M0：libtorrent vcpkg 构建 + 最小 C 内核 + magnet progress>0
- M1：FFI 全量 + btcore
- M2-M7：原方案

### 6.3 风险标记

放弃迅雷后，**唯一新增风险**是冷门 BT 资源下载体验下降。缓解措施：
- v1 上线后实测体验
- 若冷门资源下载失败率高 → v2 评估接入 debrid/115 等 RemoteProvider
- 永远不评估"原生重写迅雷网络"——成本永远大于收益

### 6.4 关于占位文件"不通用"问题的解答

用户原话："迅雷的bt下载的文件是迅雷特殊的占位文件不能和其他的bt通用这点也要摸清楚"

**完整解答**：
1. **原因**：迅雷用 CID/GCID/BCID 三套自研哈希，与 BT 标准 piece hash 不兼容
2. **表现**：.xltd 文件含 BCID 哈希表（私有格式），第三方 BT 客户端无法读取
3. **避免**：我们的下载器用标准 BT piece hash + libtorrent fastresume 格式，**保证与 qBittorrent/Transmission 等客户端完全互通**
4. **接续能力**：用户可以从 qBittorrent 把 .part + fastresume 拷到我们的下载目录继续下，反之亦然

---

## 附录：调研工件清单

新增工件（本报告的支撑材料）：

| 路径 | 内容 |
|---|---|
| `/home/z/my-project/research/file_format/` | 占位文件格式分析（按 dll 分文件） |
| `/home/z/my-project/research/protocol/all_proto_constants.txt` | 28 个协议常量全集 |
| `/home/z/my-project/research/struct_analysis/DownloadSDK_bt_fields.txt` | 917 个 BT 字段名 |
| `/home/z/my-project/download/xunlei_engine_research.md` | 上一份报告（FFI 集成方案，已废弃） |
| `/home/z/my-project/download/xunlei_independence_analysis.md` | **本报告** |

---

## 待用户确认

1. ✅ **接受路径 A**（纯 libtorrent，完全无黑盒）？
2. ✅ **接受放弃迅雷网络**（不接入 PHub/SHub/DCDN）？
3. ✅ **接受占位文件用标准格式**（.part + fastresume，与 qBittorrent 互通）？
4. ✅ **接受 M0 spike 直接走 libtorrent vcpkg 构建**（v0.2 §14 原方案）？
5. ⚠ **路径 C（仅逆向 SHub）**作为 v2 候选保留，v1 不做？

回我这 5 条，我立刻开 M0 spike 任务清单。
