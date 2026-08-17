# P2P 网络侦察审查记录（2026-08-17）

> 对象：逆向 AI 按 `p2p_network_recon_task.md` 执行的 P0-P3 侦察汇报
> 产物：`xunlei_p2p_recon_report.md`（824KB 合集，逆向 AI 环境导出）
> 审查：主项目 agent —— 采纳 P3-C 结论，但**部分"实锤证据"需降级/弃用**（见 §3）

## 1. 结论决议

**✅ 接受 P3-C：放弃接入迅雷私有 P2P 网络，主项目维持路径 A（纯 libtorrent）+ D（转换器）。**
与既有决策 D-2026-08-16-04（路径 B 推荐度 ⭐ 最低）一致，P2P 侦察没有改变主项目方向。

## 2. 可靠证据（可入库引用）

| 证据 | 级别 | 依据 |
|---|---|---|
| 迅雷是中心化+混合架构：peer 发现主要靠 PHub（中心化 hub），标准 DHT 占比 <10% | B+ | PAM 2012 学术论文（IEEE 出版）+ 本合集服务器清单交叉 |
| PHub/SHub/DPHub 主机清单（hub5p/hub5btmain/dphub/viphub5pr…） | B | 合集 §2.3（15 个 sandai 主机名；与之前研究一致） |
| PHub 类访问依赖 `captcha_sign`（随 App 版本变，盐数组维护成本高） | B | 合集云盘 API 调研 + alist ThunderExpert 生态共识（公开实现） |
| 接入工程量估算 50k-80k LOC（52 Cmd + 15 PHub + 11 SHub + 56 uDT + 哈希体系） | B | 合集路径 B 评估（与我们 D 系列估算同量级） |
| 公网无第三方接入迅雷 P2P 的先例（GitHub 33 次搜索 0 命中；现成项目全是 API/DB 层包装） | B+ | 逆向 AI 调研 + 主项目 web 复核（ThunderPlatform/XLP 逆向均为单服务逆向，非全网络接入） |

## 3. 审查发现问题（**禁止引用为 A 级**）

汇报声称的以下"反汇编实锤"在产物合集中**无对应物**，且与已知事实冲突：

| 声称 | 问题 | 处置 |
|---|---|---|
| message_id 表（Choke=5/Have=7/BitField=4…"BEP id 重排"） | 与标准 BEP-3 全错位；且 PunchingHole 与 SuggestPiece 共用 0x16——wire 上两消息不可能同 id。迅雷 BT 层已实测与标准完全兼容（infohash 校验 + 标准 piece SHA1 1866 命中），不可能重排 wire id。此表更像内部枚举/vtable slot 序号误读 | ❌ 弃用。**不能用"迅雷重排了 BT 消息号"**（与事实直接矛盾） |
| `XPF_AES*` 6 函数 + `rc4_handler` 自实现 RC4/AES-ECB | 合集中搜不到这些符号（grep 0 命中）；命名风格可疑；"AES-ECB 密钥内嵌消息前 8 字节"无来源 | ⚠ 未证实。加密判定维持"私有加密存在但细节未确认"（B-），**不得写死 ECB 细节** |
| "PAM 2012 论文印证 AES-ECB" | 论文（IEEE PAM 2012《Unreeling Xunlei》类）公开摘要未提 AES-ECB 细节；无法核验 | ❌ 弃用该"印证"表述 |

**根因提示**：逆向 AI 沙箱路径（/home/z/...）的中间产物未随汇报落盘（产出物清单中 FINAL_REPORT/xbtpackage_vtables.json 等缺失）；汇报中的部分推导可能是渲染/幻觉。**结论采纳、细节存疑**。

## 4. 对主项目的影响

- 无：主项目 v1 本就 = 纯 libtorrent + 转换器 + 云兜底（默认关），P2P 线关闭
- 迅雷/比特彗星长效种子类"私有加速"统一判定：不接入（成本/法律/维护三重否决）
- 若未来反悔：可复用本合集的服务器清单/captcha_sign 分析做 PHub 通道研究（独立立项，非下载器本体）

## 5. 更新

- D-2026-08-17-02（DECISIONS.md）：接受 P3-C，P2P 侦察关闭
- RESEARCH_STATE.md / NEXT_ACTION.md 同步

## 6. 中间产物补齐归档（2026-08-17 晚）

用户从云端分析环境下载了全部产出（`~/Downloads/p2p_research_complete*.md` 等），本次补齐入库：

**新增（补 §3 缺失项）**：
- `FINAL_REPORT.md` / `PROGRESS_REPORT_v2/v3.md` / `PUBLIC_INTEL_REPORT.md` / `RESEARCH_STATE.md`
- `xbtpackage_vtables.json` / `phub_shub_cmd_analysis.json`（§3 审查时"搜不到"的符号表——现在物证在库，可复核）
- `scripts/`（disasm_* / poc_phub_http_v2-v4 / test_captcha_sign*，capstone/ghidra 反汇编脚本）
- `alist_src/*.go`（**alist AGPL-3.0 源码节选，仅作研究参考，不并入主程序**）
- `p2p_research_complete.md`（最新完整版，含 P2 协议文档化 + P3 集成路径可行性评估 + "16 PoC 全部失败 → 下一步 Wireshark 抓 1 个真实 PHub POST" 结论）
- `p2p_research_complete_v1.md`（前版，含独立 Http.dll 反汇编报告章节，v2 已重组整合）
- `p2p_recon_complete.md`（10:36 早期合集：RESEARCH_STATE + PROGRESS v2/v3）
- `xunlei_independence_analysis.md` / `xunlei_engine_research.md`（08-16 两篇决策依据，原在沙箱缺档）

**证据分级（维持既定决策，不变）**：以上全部为逆向 AI 中间产物——**B-/C 级参考，禁止引用为 A 级**。§3 的三条"禁止引用"结论维持：message_id 重排表弃用、XPF_AES/RC4 未证实、PAM 论文 AES-ECB 印证弃用。新物证可复核 §3 存疑点，但**不得据此升级**；A 级需要真实抓包（Wireshark/pktmon 路径，用户侧可选）。

**去重**：`xunlei_p2p_recon_report.md`（805KB）与 `xunlei_research_complete.md` 哈希一致，删除前者，统一指向后者。

## 7. 真实抓包实测（2026-08-17 晚，闭环验证）

研究建议"抓 1 个真实 PHub 请求破解 body 格式"。主项目侧用 **pktmon 全量抓包**（免安装，Windows 自带）实测：

- 步骤：用户以管理员跑 `scripts/research/capture_phub.ps1`，打开迅雷下载 BT 任务，全量捕获 ~30s（62 万包，pktmon 丢 104 万事件——全量模式下性能不足，仅作参考）
- **结果**：80 端口 **0 条流**；443 端口 39 条流，出站主目标 **104.17.186.65（Cloudflare）** + 180.163.54.163；另有 BT 协议流（180.114.103.36:20011 "BitTorrent protocol"、DHT/peer 6881、DNS DoH 223.5.5.5/1.12.12.12）
- **判定**：新版迅雷 PHub 走 **443 TLS**（"POST / HTTP/1.1 明文"模板属旧版/其他路径）；body 在 TLS 密文内，应用层还有 ParamStream/AES（§3 未证实项，格式未破）。
- **结论**：**建议的"抓 :80 POST body"路径实测不成立**。外部抓包拿不到明文 body（传输 TLS + 应用双层加密；中间人需绕过迅雷证书校验/SSL pinning，成本远超收益）。
- **闭环**：P3-C（放弃接入）获实测反面支撑——16 PoC 失败（应用层格式）+ TLS 拦截（传输层）双路汇合，与归档结论一致。**本线到此为止，不再深入**。
- 遗留可用物：BT 协议层流样本（180.114.103.36:20011）可作"迅雷 BT 层 100% 标准"的后续 A 级实证（不影响主项目，主项目已按 libtorrent 纯标准实现）。

**实测工具**：`scripts/research/capture_phub.ps1`（pktmon 全量抓包，UAC 自提权）+ `extract_phub_body.py`（纯标准库 pcapng 解析/TCP 重组）。真实抓包输出目录 `scripts/research/captures/` 已 gitignore（含用户网络流量，严禁入库）。

## 8. 内存转储实证（2026-08-17 深夜，A 级升级）

抓包被 TLS 拦截后改用**进程内存转储**（用户任务管理器 dump `DownloadSDKServer`，355MB；脚本 `scripts/research/scan_minidump.py` 本地扫描、自动脱敏，dump 文件不出本机）：

**逆向研究关键结论逐一在真实进程内存中命中（实物存在性 A 级）**：
| 逆向声称 | 内存实物 |
|---|---|
| QAClient 注册 QAClientPackage + XDL_QAClientPackageParser + UdpConnection.HubClient | ✅ 三字符串同现（0x33159cf 注册表区）；`QAClientPackage` 6 处、`XDL_QAClientPackageParser` 3 处 |
| "POST / HTTP/1.1" + "Content-type: application/octet-stream" 模板 | ✅ 与 QAClientPackage/Parser 同区（0x11f66e13） |
| PhubHttpPkgRequester 类 | ✅ MSVC RTTI `?AVPhubHttpPkgRequester@@` + `?AVPhubAllResHttpPkgRequester@@`（0x11fec8b7） |
| XPF_ParamStream 系列序列化 API | ✅ 85 处符号：`XPF_ParamStreamWrite/ReadPointer/UInt32`、`BeginEnum`、`XPF_CreateParamStream[WithBindBuffer/WithBuffer]` |
| ConfigHub 下发（AES/RSA 密钥来源） | ✅ 配置键名表：`ConfigHub/ConfigHubHost/ConfigHubPort/VersionIDFromCfgHUB` + **`UseRSA`**（0x11f427b3） |
| PHub 主机/端口族 | ✅ `P2PHubHost/pr-phub.sandai.net`、`P2PHubIPv6Host/pr-v6-phub.sandai.net`、`P2PHubPort/P2PHubUdpPort`、`AllHubPort/MagnetHubPort/BTIndexHubPort/TrackerHubPort` |

**新增情报**：`vip_dcdn_token`/`vip_dcdn_token_backup`/`equity_token`/`qaclient_maxpackagesize`/`qaclient_maxrecvsize`（PHub 参数名）；构建路径泄露 `D:\jenkinsAgent\...\Downloadlib_33.2`；`UdpConnection.HubClient` 提示 PHub 除 HTTP 外存在 **UDP 通道**（P2PHubUdpPort）。

**结论**：逆向研究 B/C 级发现升级为 **A 级（真实进程实物）**；但 **QAClientPackage 序列化 body 明文未取得**（需堆追踪/注入/时序对齐，工程量大，且仍在"接入"成本线上）。**P3-C 结论维持**：协议存在性已 100% 实证，接入成本/风险不变——收益仍不匹配。**研究线正式关闭**（三轮方法：PoC 失败→抓包 TLS 拦截→内存实证，已成闭环）。

**方法学沉淀（可复用）**：pktmon 免安装抓包 + minidump 本地扫描（脱敏），对任何"自家进程私有协议"研究适用；工具在 `scripts/research/`。