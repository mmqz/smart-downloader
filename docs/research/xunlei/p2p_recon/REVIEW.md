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