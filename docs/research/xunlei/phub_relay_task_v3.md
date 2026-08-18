# PHub/SHub 加密链路 · v3 反馈与追加任务（给云端 RE AI）

> **来源**：本地（用户直连环境）对 v2 交付成果（`phub_peer_accelerator_v2.zip`：
> AES key = `MD5(seq_no_LE || cmd_id_LE)`、AES-128-ECB、4 个 RSA-1024 服务公钥）
> 在**真实抓包样本**上的逐项验证结果 + 追加逆向任务。
> 版本：2026-08-18 晚。前置文档：`phub_relay_task_v2.md`（§9 校准 + §10 重放实验）。

---

## 1. 底线结论（先读这个）

| 项 | 状态 | 证据 |
|---|---|---|
| 368B SHub 请求结构 = RSA ekey 路径 | ✅ **A 级实锤** | 8+4+128+4+224=368 精确吻合（见 §2） |
| SHub 响应/请求解密用 MD5(seq\|\|cmd) | ❌ **证伪** | 全部 (cmd,seq) 变体解 2692B 响应 = 高熵垃圾（见 §3） |
| PHub forge（13B 头 + MD5-AES）可发 | ❌ **被服务器拒** | pr-phub:80 回 `decrypt request failed.`（见 §4） |
| dump 内存可捞响应 key/明文 | ❌ **无果** | ±4KB 10000 候选 16B key + TLV 全扫均无命中（见 §5） |
| 协议无强防重放（v2 重放结论） | ✅ **保持** | 4 次重放同请求→同响应（2692B 相同） |

**含义**：客户端侧**没有 RSA 私钥、不能导出响应 key** → 纯离线解密路径
（MD5 派生/内存扫描）**不可闭环**。真正可执行的闭环只剩两条，见 §7。

---

## 2. 368B SHub 请求结构（准确版，替代 v2 §3.3 的"差 4"猜测）

```
Offset  Size  Field                   值（实测）
------  ----  ---------------------   ---------------------------------
0       8     header (cmd/flag/seq)   88 58 03 26 10 27 00 00
8       4     ekey_size               80 00 00 00 = 128（= RSA-1024 密文长）
12      128   ekey (RSA 包裹的 AES key) af 94 0a 31 ...（高熵）
140     4     aes_body_size           e0 00 00 00 = 224
144     224   AES-128-ECB 密文          （14×16，对齐）
```
- **总数：8 + 4 + 128 + 4 + 224 = 368**（与样本文件长度完全一致）。
- 请求的 AES key **不是** MD5 派生，而是客户端随机生成、经 RSA 公钥加密为 ekey 上送。

## 3. MD5 派生全面证伪（SHub 侧）

对 2692B 响应（4B 长度 + 2688B 密文）逐一尝试以下 key 构造，**全部输出高熵垃圾**：

- `MD5(seq_LE || cmd_LE)` / `MD5(cmd_LE || seq_LE)`（cmd/seq 取 368B 头部
  offset 0/4/5 的各种 LE/BE 解读）
- `MD5(请求头 9B/13B 原始前缀)`（原样哈希）
- cmd=1 / cmd=0x26035888 / seq=0x27 / 0x80000027 等组合

→ **SHub 请求/响应都是 RSA 会话 key 路径，MD5 派生不适用于 SHub**。

## 4. PHub forge 实测被拒（新情报，需云端解读）

环境：`pr-phub.sandai.net:80` 直连可达（IP：180.163.56.147 / 123.182.51.211，DNS 正常）。

发送（各 1 次，`POST /`，Content-Length 精确）：
1. `13B 头（cmd=1 flag=0xb seq=0x0A000001 enc_len=80）+ MD5(seq||cmd) 加密的 64B 明文`
2. 同款但 key = `MD5(cmd||seq)`
3. 明文未加密对照（13B 头 + 明文 64B）

**响应一律相同**：`23B` 明文 `decrypt request failed.`（十六进制 `646563727970742072657175657374206661696c65640a`）。

**云端待解读的问题**：
- Q1：服务器"解密请求失败"是在**哪一步失败**？RSA ekey 解析？AES 解密？还是头域校验？
  → 若能看到 `PhubHttpPkgRequester::BuildPacket` / 发包路径的反汇编，确认 **PHub 请求是否
  也含 ekey 字段**（如果 PHub 也为 `[8B 头][ekey_size][ekey][aes_size][AES]` 结构，
  则"MD5 派生 forge"整条线作废，直接走 §7 hook 路线）。
- Q2：v2 §3.1 的"13B PHub 头"（cmd_id/flag/seq_no/enc_len）与实测 8B 头的关系？
  是否 PHub 与 SHub 头布局不同，还是云端 13B 头推断自其它代码路径？

附：36B 响应样本 `20 00 00 00 16 fa db bf ...`（= 4B 长度 32 + 32B 密文，
无 13B 头）；3252B 响应 `b0 0c 00 00 ...`（= 4B 长度 3248 + 3248B 密文）。均无 13B 头
→ 响应侧与 v2 §3.4"4B 长度 + 纯密文"分支一致。

## 5. dump 内存扫描结果（可跳过）

- minidump_scanner 在请求密文 ±4KB 内提取 10000 个 16B 候选 key，逐个试解 2692B：
  **无明文命中**（6 个 zlib 命中最长 135B，均为误报小流）。
- TLV 全量扫描 Clash-off dump：仅 ≤56B 小结构，**无响应解密明文残留**。
- 结论：响应明文生命周期极短，离线 dump 不可复用。

## 6. 已可交付/可复用的材料

- 4 个 RSA-1024 公钥 PEM（key1..4）：分类（P2S/DPHub登录/PHubQueryRes/AllResQuerypeer）**建议保留**，
  将来拿到 ekey↔key 对时用于验证 RSA 填充与密钥协商方式。
- 重放工具链：捕获请求密文原样重放 **长期有效**（无防重放）——加速器"请求侧"已通，
  差"响应解密"。

## 7. 给云端的追加任务（按优先级）

1. **【最高】反汇编 `PhubHttpPkgRequester`（HQ 请求组装路径）**：确认 PHubQueryRes
   请求体是否包含 ekey（RSA 包裹）→ 回答 §4 Q1/Q2。若 PHub 也是 ekey 路径，
   请在任务包中**明确宣布"MD5 派生 forge 不可行"**，避免后续轮次浪费。
2. **【高】反汇编 36B/3252B 响应解密路径在 Http.dll 的调用点**（SR_SHUB/PHub 响应
   处理）：确认响应 AES key 的真实来源（= RSA 解密 ekey 得到的会话 key？
   还是服务器固定 key？）。若为会话 key → 解密必须拿到 ekey 对应私钥或运行时 key，
   即确认 §7.3 是唯一路径。
3. **【中】Frida hook 方案设计**（若 sandbox 有真实迅雷 Windows 环境可先自测）：
   - hook 点：`XPF_AESDecryptBufferECB`（P2PBase.dll，导出）、`XPF_MD5HashData`、
     PHubHttpPkgResponser::ParseData
   - 抓取：调用时 (key 16B, 密文 buf, 长度, 明文 buf) → 直接落地 (key, 密文, 明文) 三元组
   - 一次真实下载会话即可闭环：请求构造（重放或新抓）+ 响应解密 + peers 明文解析。
   若云端环境不具条件，用户侧可配合（本地装有迅雷 + 可 dump），需提供 hook 脚本骨架。

## 8. 边界与安全

- 本任务包**不含**任何账号 token/设备标识（equity_token 等均打码）。
- 目标仍为 peer 加速器（数千行），非 134 消息全套。
- 加速器核心依赖"响应解密"——**在此之前，任何进一步协议枚举均为低优先级**。

---

## 9. 云端 v3 回执（2026-08-18 晚）——解密路径定案

交付：`phub_peer_accelerator_v3.zip`（PHUB_PROTOCOL_SPEC_V3.md + `phub_capture.js` Frida hook）。

**Q1 答复**（forge 被拒原因）：拒绝发生在 **RSA ekey 解密步**——forge 包缺
`[4B ekey_size=128][128B RSA ekey]`，服务器解不出 AES key。

**Q2 答复**（13B vs 8B 头）：13B 头来自 `PhubHttpPkgResponser::ParseData @ 0x1801618a0` =
**非生产路径**（DPHub UDP / 旧版兼容）；生产请求 = **8B 头 + 4B ekey_size + 128B ekey +
4B aes_size + AES body**（368 精确）。

**v3 修正表**（云端反汇编 `DownloadSDK.dll @ 0x180285de0`，唯一 RSA+AES 同现函数）：
| v2 断言 | v3 修正（A 级） |
|---|---|
| AES key = MD5(seq‖cmd) | ❌ = `XPF_RandomBytes` 生成的 **16B 随机数**（@ 0x180285fbe） |
| PHub 用 13B 头 | ❌ 8B 头 + RSA ekey + AES body |
| MD5 forge 可行 | ❌ 需 RSA 私钥（服务器持有） |
| MD5 离线解密可行 | ❌ 必须运行时捕获随机 AES key |
| cmd_id = 0x26035888 | ✅ 硬编码 @ 0x180286047，与 368B 样本吻合 |

MD5 派生仅存于 5 个非 PHub 路径（0x18015b090 / 0x180162dc0 / 0x1801672d0 /
0x180177150 / 0x1802aca80）。

**定案：唯一可执行闭环 = Frida hook**（本地用户侧）：
1. `XPF_RandomBytes` → 捕获 16B 随机 AES key（生成时）
2. `XPF_AESCreateDecryptContext` → 同一 key 进入响应解密
3. `XPF_AESDecryptBufferECB` → (ctx, ct, len, out) 三元组，**明文在 out 里**

`phub_capture.js`（云端交付，已解压至 `cloud_delivery/v3/`）：hook 7 个导出函数、
懒安装、落盘 `C:\phub_capture\`。**待用户侧运行**：装 frida → 迅雷下载时挂 hook →
拿 (key, ct, pt) → `decrypt_with_captured_keys.py` 批量解 → peers 明文 → 加速器闭环。

**主项目侧现状**（不阻塞）：magnet 接入 BT 引擎（btcore `add_magnet` 已就绪）推进中。