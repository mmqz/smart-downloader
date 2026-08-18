# PHub 协议逆向接力任务包 v2（2026-08-18 凌晨）——真实样本 + 加密结构破解

> 相比 v1：新增**真实密文样本**（Clash 关闭后直连抓取）、**加密流程实锤**（RSA+AES）、
> **RSA 公钥**、**鉴权 token 格式**、**消息结构规律**。目标仍是"PHub peer 加速器"。

## 0. 一句话现状

**17 PoC 失败的根因已定**：body 非纯 ParamStream 明文，而是 **AES 加密 + RSA 包裹密钥**，
且**密钥运行时生成/ConfigHub 下发**（静态 DLL 无 key）。**真实样本已到手，缺最后一块：RSA 公钥与字段精确定义。**

## 1. 真实密文样本（本轮新，Clash 关闭直连 / 以及 Clash 开启对比）

样本已导出为 bin 文件（hex 可直读）：
| 样本 | 大小 | 首 8B | 说明 |
|---|---|---|---|
| REQUEST_POST_sr-shub | 368B | `88 58 03 26 10 27 00 00` | SHub POST body（**AES 密文**，368=23×16 块对齐） |
| RESP_200_OK（36B）×3 | 36B | `20 00 00 00 16 fa db bf...` | PHub 响应（4B 长度头=32 + 32B 密文=2×16），**同一密文重复出现 3 次 → ECB/无随机 IV 嫌疑** |
| RESP_200_OK（3252B） | 3252B | `b0 0c 00 00 9b 2f dd d0...` | 直连下大响应（4B 长度头=3248 + 3248B 密文=203×16），entropy 7.18 |

**消息结构规律（高置信）**：`[4B LE 长度][AES 密文]`，密文严格 16 字节块对齐。

## 2. 加密流程实锤（反汇编/符号证据）

`ReportSender2`（DownloadSDK）代码区字符串：
- `ReportSender2::crypt_aes_key` → `PEM_read_bio_PUBKEY` → `EVP_PKEY_get1_RSA` →
  `RSA_public_encrypt` → `RSA_size failed` / `RSA_public_encrypt failed`
- `ReportSender2::crypt_data_package`
- 请求字段名：`appid=` `t=` `gzip=` `data_list=` `ekey=` `pkv=`

**结论**：请求体 = `appid/t/gzip/data_list/ekey/pkv` 结构，其中
`ekey` = RSA 公钥加密的 AES key，`data_list` = AES 加密的数据（含 ParamStream body）。
**UseRSA 配置键 = 此开关。**

## 3. RSA 公钥（本轮新）

`scripts/research/captures/keys/`（本地）：
- `pub_1.pem` / `pub_2.pem`：**相同 RSA-2048 公钥**（PEM，450B），`pub_2` 位于
  `ReportSender2::crypt_aes_key` 代码区旁（0x1082bede）——**即加密用公钥**
- `priv.pem`：`-----BEGIN RSA PRIVATE KEY-----`（1704B，0x118df4ed）——**用途待确认**
  （可能是迅雷测试/自签对，也可能是 ReportSender 密钥对）——**若与 pub 配对，可直接解 ekey**

> 云端任务：① 确认 pub 与 priv 是否配对（modulus 对比）；② 反汇编 crypt_aes_key
> 调用点确认公钥来自哪里（ConfigHub 下发？内置？）；③ 若配对 → 用 priv 解 ekey → 拿 AES key → 解 368B 请求密文 → 得到 ParamStream body 明文 → 100% 闭环。

## 4. 鉴权（本轮新，敏感已打码）

运行时内存明文（多次出现）：
```
tokenmode=benefits
tryqueryallhuberror=8472            ← PHub 查询失败累计 8472 次（fake-ip 时期）
tryqueryallhuberror_withtoken=false
vip_insert_dcdn=0
qaclient_maxrecvsize=0
qaclient_maxpackagesize=0
equity_token=<REDACTED-25位数字>,token_mode=2,app_guid=<REDACTED>
```
- 字段格式 = **逗号分隔 k=v 字典**（PHub 参数序列化格式之一）
- `vip_dcdn_token` / `vip_dcdn_token_backup` 键存在但值空（需 vip 才填）

## 5. ConfigHub 链（本轮确认）

- `ConfigHubHost=hub5p.sandai.net`（键名表，3 处）——**云端沙箱 DNS 污染不可达；用户直连环境可解析**（180.163.56.x 段）
- 配置键名表：`ConfigSignature.UpdateIntervalMinute` / `VersionIDFromCfgHUB` /
  `ConfigHub:VersionIDFromCfgHUB` / `ConfigHub:ConfigSignature` / `ConfigHub:LastTimestamp`
- `XSDN:ResourceIdPrefix` / `XSDN:Channel` 等默认配置（`XSDN:`/`ConfigHub:` 前缀键值表格式）

## 6. 环境教训（重要）

- **Clash fake-ip**（198.18.0.0/15）会污染 DNS：`pr-phub.sandai.net → 198.18.0.124`，
  流量走代理，抓包/分析失真。**真实直连解析 = 180.163.56.147**。
- 之前"80 端口零流量/443 TLS"结论需修正语境：TLS 是传输层（真实），
  **但应用层密文样本 = 4B 长度 + AES**（与 TLS 无关的第二层加密）。
- TLV 全量扫描误报率高（tag 字节太常见）→ 用锚点法（模板/键名定位后局部提取）。

## 7. 下一步（云端可执行，按优先级）

1. **priv/pub 配对验证 + 解 368B 请求密文**（若配对成功 = 一键闭环）
2. 反汇编 `crypt_data_package` 确认 AES 模式（ECB/CBC/GCM——36B 重复 + 16 对齐 → ECB 嫌疑大）
3. 反汇编 PHubHttpPkgRequester 的 body 组装（ParamStream 字段顺序 + appid/pkv 取值）
4. 响应 3252B 解密后解析（预期含 peer 列表 → peer 加速器核心）
5. 用户侧（可后置）：真实直连下重抓"POST 请求窗口"（equity_token 可用时）

## 8. 边界

- 目标仍为 peer 加速器（几千行），非 134 消息全套
- token/密钥属用户迅雷账号敏感材料：**任务包外不外传**，分析输出打码
- 样本 bin 在 `scripts/research/captures/`（gitignore，不入库）

## 9. 本地实测校准（2026-08-18 凌晨，用户侧验证）

**9.1 priv/pub 配对验证：失败（不同模数）**
- 提取的 RSA-2048 公钥（ReportSender2 代码区旁）与私钥**非同模**，不配对。
- 判定：公钥属 **ReportSender2 上报组件**（crypt_aes_key/crypt_data_package 是
  数据上报加密，**非 PHub 请求加密**）；私钥用途不明（非 PHub 服务端'），
  **云端勿用此对解 ekey，会浪费轮次**。

**9.2 368B 请求密文结构观察**
- 头部存在小整数标记：offset8 `10 27 00 00`(=10000)、offset12 `80 00 00 00`(=128)、
  offset0x8c `e0 00 00 00`(=224)；0x140 起 224B 高熵密文；整体 368=23×16。
- 判定：**多段结构**（长度/计数头 + 分块密文），非单一 RSA 密文。
  对照物：云端已有的 `SERIALIZE_FN`（0x1803192a0）反汇编。

**9.3 36B 响应密钥流特征**
- 同一密文在内存 3 处重复（不同时刻）→ **同一输入得同一输出 = ECB 或固定 IV**，
  云端解密时**优先试 ECB**（无需 IV），其次 CBC + 猜测 IV 来源。

**9.4 明文生命周期线索（供云端反汇编聚焦）**
- PHub body 组装在 PhubHttpPkgRequester（`?AVPhubHttpPkgRequester@@` RTTI 实锤）；
  序列化后端点在 WsHub/HTTP 缓冲（368B 密文旁有 POST 模板 + Host: sr-shub.sandai.net）。
- 若云端需要代码段：用户侧可从 dump 提取模块代码交付（files 提取工具可加）。

## 10. 重放实验（2026-08-18 凌晨，决定性）——固定 key 实锤

**实验**：把 Clash-on dump 捕获的 368B 请求密文原样 POST 到 `sr-shub.sandai.net:80`：
- **4 次重放全部 200 OK**（`Server: elb`，实时 Date 头）
- **4 次响应完全相同**（2692B，md5 一致，`80 0a 00 00` = 4B LE 长度 2688 + 密文）
- 响应样本：`scripts/research/captures/replays/resp_0..3.bin`

**结论（A 级）**：
1. **协议无强防重放**——捕获的有效请求密文可直接复用（peer 加速器可原样重放）
2. **响应用固定 key 加密**（相同请求 → 相同密文；无随机 IV/时间戳盐）——**ECB 或固定 IV**
3. 响应格式与 3252B 一致：`[4B LE 长度][AES 密文 16B 对齐]`（2688=168×16）

**给云端的闭环路径（按优先级）**：
1. **反汇编响应解密函数**（Http.dll 内 SR_SHUB 响应解析路径）→ 确认响应 key 派生/固定 key
   → 解 2692B 响应 → 拿 peers（加速器核心数据）
2. **构造已知明文对**：云端用其 sandbox 的 crypt 实现（crypt_aes_key/crypt_data_package）
   构造它自己的合法请求明文 + 发送 → 服务器响应密文 → 已知明文攻击恢复固定响应 key
   （响应明文结构可通过反汇编响应解析函数获得）
3. **篡改观察**：用户侧可做密文翻转实验（改 368B 某些字节重发）→ 服务器 200/4xx 分布
   → 判断校验强度（补充情报，非必需）

**本地工具链（均可交付/复用）**：
- `dump_disasm.py`（minidump 模块定位 + 任意 RVA 反汇编，capstone）
- `pe_iat_probe.py`（PE 导入/导出表解析 → IAT 槽运行时地址）
- `extract_http_body.py` / `analyze_body.py` / `replay_body.py` / `rsa_probe.py` / `scan_paramstream_body.py`

## 11. 云端 v2 破解成果 · 本地实测反馈（2026-08-18 晚）

云端交付 `phub_peer_accelerator_v2.zip`（算法：`AES_key = MD5(seq_no_LE || cmd_id_LE)`、
AES-128-ECB、4 个 RSA-1024 服务公钥）。本地对**真实样本**逐项验证：

**11.1 368B SHub 请求结构——实测精确确认（A 级）**
```
[0:8]   88 58 03 26 10 27 00 00    8B 头（cmd/flag/seq 区）
[8:12]  80 00 00 00                ekey_size = 128（RSA-1024）
[12:140] af 94 0a 31 ...           128B RSA 包裹的 AES key（ekey）
[140:144] e0 00 00 00              aes_body_size = 224
[144:368] ...                      224B AES 密文（14×16）
```
8+4+128+4+224 = **368 精确吻合**（云端 §3.3 的"差 4"猜测已修正）。
→ **SHub 请求 = RSA ekey 路径，不是 MD5 派生**，符合云端 §3.3 暗示。

**11.2 MD5(seq||cmd) 派生对 SHub 响应实测失败**
- 用 368B 解出的全部 (cmd, seq) 候选组合（offset0/1/4/5 各种 LE/BE 变体）试解 2692B 响应：
  **全部为高熵垃圾**，无可读明文 → SHub 响应同样走 RSA 会话 key，**非固定 MD5 key**。

**11.3 PHub forge 实测被拒（关键情报）**
- `pr-phub.sandai.net:80`（直连 180.163.56.147/123.182.51.211）对云端规格的
  `13B 头 + MD5-AES` forge 包（含未加密对照）**一律返回 23B 明文**
  `decrypt request failed.` → **PHub 很可能与 SHub 同构（RSA ekey）**，
  MD5 派生仅在响应解密侧成立（需云端再验证 PhubHttpPkgRequester 是否含 ekey 组装）。

**11.4 dump key 扫描无果**
- minidump_scanner 在请求密文 ±4KB 提取 10000 候选 16B key 试解 2692B：无明文命中
  （zlib 命中共 6 个全是误报小流）。
- TLV 全扫 dump 2：仅小结构（≤56B），无响应解密明文残留 → 明文生命周期极短。

**11.5 诚实结论（给云端）**
- 结构破译、公钥分类、重放性 = **已闭环**；**解密 = 未闭环**。
- 障碍：RSA 私钥（服务端持有）+ 响应 key 在客户端侧不可导出（非内存常驻明文）。
- **客户端侧唯一可行路径 = 运行时 hook**：hook `XPF_AESDecryptBufferECB`
  （P2PBase.dll @ RVA 0x1a8d0 附近）或 `XPF_MD5HashData`，在真实迅雷进程中
  抓 (key, 密文, 明文) 三元组 → 一张真实会话即得加速器全部密钥材料（含响应明文 peers）。
- 或云端反汇编 `PhubHttpPkgRequester` 请求组装确认 **是否 ekey 路径**（若 PHub
  也为 ekey，则"MD5 派生 forge"整条线不可行，聚焦 hook 路线）。

**本地新增工具**（`scripts/research/`，均可交付）：
- `forge_phub_test.py`（forge + 直连 oracle 验证）
- `try_key_orders.py`（key 顺序暴力 oracle，含明文对照）
- `decrypt_2692_variants.py`（MD5 派生组合全试）
