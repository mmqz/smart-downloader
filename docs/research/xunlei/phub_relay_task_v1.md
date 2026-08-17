# PHub 协议逆向接力任务包 v1（2026-08-17）

> 用途：交给云端逆向 AI 继续破解。本包包含**真实进程内存实证**（A 级）的完整
> 锚点清单 + 待破解点 + 建议方法。目标是**最小可行目标**（见下），不是完整接入。

## 0. 目标（明确缩小，成功标准可判）

**实现一个"PHub peer 加速器"**（而非完整接入迅雷私有网络）：
- 最小目标：构造 1 个**被服务器接受**的 PHub HTTP 请求（当前所有 PoC 返回
  "decrypt request failed"），并解析出响应中的 **peer 列表**（假设响应是标准
  BT peer IP:port 列表——迅雷 BT 层已 A 级证实为标准 BEP，此假设概率高）
- 成功标准：PHub 返回 200 + 非 "decrypt request failed" + 解析出 ≥1 个 peer
- 工作量评估：若仅需 PHub 客户端 + token + 加密 + peer 解析，预计几千行
  （非 134 种消息全实现的 5-8 万行）

## 1. 已确认的 A 级实物证据（来自真实进程内存转储）

来源：用户机器 `DownloadSDKServer` 进程 355MB minidump（迅雷下载引擎服务），
本地扫描（脚本 scripts/research/scan_minidump.py），以下全部为**真实运行内存中的
字节实物**（非反汇编推断）。

### 1.1 类/注册（MSVC RTTI + 字符串）
- `QAClientPackage`（包类型名，6 处）
- `XDL_QAClientPackageParser`（解析器，3 处）
- `UdpConnection.HubClient`（连接类型注册，与 QAClient 同注册表区）
- `?AVPhubHttpPkgRequester@@`、`?AVPhubAllResHttpPkgRequester@@`（RTTI 类名）
- `QAClientPackage` 与 `POST / HTTP/1.1` + `Content-type: application/octet-stream`
  字符串**同区相邻**（PhubHttpPkgRequester 的模板区）

### 1.2 ParamStream API 完整清单（85 个符号，XPF 导出）
- `XPF_ParamStreamWritePointer` / `XPF_ParamStreamWriteUInt32` / `XPF_ParamStreamWriteInt32`
- `XPF_ParamStreamReadPointer` / `XPF_ParamStreamReadUInt32` / `XPF_ParamStreamReadInt32`
- `XPF_ParamStreamBeginEnum` / `XPF_ParamStreamEndEnum`
- `XPF_ParamStreamRelease` / `XPF_ParamStreamReset` / `XPF_ParamStreamGetBuff`(待确认)
- `XPF_CreateParamStream` / `XPF_CreateParamStreamWithBindBuffer` /
  `XPF_CreateParamStreamWithBuffer`
- 建议：从 dump 提取这些函数的**实现字节**反汇编（符号表 → 代码段），
  直接得出序列化布局（字段类型 0-16 跳转表逻辑）

### 1.3 配置键名表（ConfigHub 下发配置的 schema 键）
- `P2PHubHost`=pr-phub.sandai.net / `P2PHubIPv6Host`=pr-v6-phub.sandai.net
- `P2PHubPort` / `P2PHubUdpPort`（**UDP 通道存在**）/ `P2PHubIPv6Port`
- `AllHubPort` / `ServerResourceHubPort` / `BTIndexHubPort` / `IndexHubPort` /
  `MagnetHubPort` / `TrackerHubPort`
- `ConfigHub` / `ConfigHubHost` / `ConfigHubPort` / `VersionIDFromCfgHUB`
- **`UseRSA`**（应用层加密使用 RSA 的开关）
- `SdkVersion` / `ObscureVersion` / `ObscureVersionFor4`
- `vip_dcdn_token` / `vip_dcdn_token_backup` / `equity_token` /
  `qaclient_maxpackagesize` / `qaclient_maxrecvsize`（PHub 参数名）
- 用户级配置：`vip_dcdn_token=`、`equity_token=`（值在堆中，键名在 .rdata）

### 1.4 其他
- 构建路径：`D:\jenkinsAgent\workspace\Downloadlib_33.2\PC_SDK_Master_VS2019\...`
  （SDK 版本线 Downloadlib_33.2）
- 网络实测：PHub 走 443 TLS（Cloudflare 104.17.186.65），80 端口零流量
  （明文 HTTP 模板为旧版/降级路径）

## 2. 待破解点（按优先级）

1. **PHub HTTP body 序列化布局**（最高优先）：ParamStream 写出的字节序——
   header 结构、cmd/type 字段位置、字段类型 0-16 编码。方法：反汇编
   `XPF_ParamStreamWrite*` 实现（dump 有符号表锚点，能定位代码）
2. **body 加密**：`UseRSA` 存在 → 可能是 RSA 包裹对称密钥 + 对称加密 body。
   密钥来源 ConfigHub（VersionIDFromCfgHUB 提示配置带版本）。
   **历史参考：迅雷旧版弱加密先例（RC4 / AES-ECB），新版本未知，值得实测**
3. **UDP 通道**（P2PHubUdpPort + UdpConnection.HubClient）：PHub 是否存在
   UDP 探测/打洞（类似 STUN），HTTP 与 UDP 的分工
4. **鉴权**：vip_dcdn_token / equity_token 如何获取与使用（可能含时间戳防重放）

## 3. 建议方法（云端可执行）

- **A. 反汇编 ParamStream 实现**（零依赖 dump 即可）：从符号表定位
  XPF_ParamStreamWritePointer/UInt32 的代码地址 → 反汇编 → 得出字节布局 →
  构造 1 个合法 body → 用 HTTP POST 打 pr-phub.sandai.net（云端沙箱可达，
  之前已实测返回 "decrypt request failed"）
- **B. 响应对比**：用非法/合法 body 各发一次，对比响应差异（错误文案/状态码
  枚举可当 oracle：如 "decrypt request failed" vs 其他错误 → 逐步逼近）
- **C. 时序内存 dump**（若 A 不可行）：让真实迅雷下载 BT 的同时抓进程内存，
  找通信窗口内的 ParamStream buffer 明文（此步需用户配合，可后置）

## 4. 边界与约束

- 目标仅为 **peer 加速器**（PHub 拿 peer 喂给标准 BT 客户端），不实现 134 种
  消息全套
- 结果分级：反汇编产出 = B 级；真实请求成功 = A 级（需用户侧实测）
- 主项目（smart-downloader）不依赖本线；本线产出作为可选加速插件（独立仓库）
