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
