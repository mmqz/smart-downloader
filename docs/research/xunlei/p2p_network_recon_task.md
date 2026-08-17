# 任务指令: 迅雷私有 P2P 网络接入可行性逆向 (第 2 层加速)

> 对象: 逆向 AI (capstone 反汇编 + 抓包 + 协议复现)
> 发起: 主项目 (Rust + libtorrent 多引擎下载器, M0/M1 已完成)
> 日期: 2026-08-17

## 1. 任务目标

回答并实现: **第三方下载器能否接入迅雷私有 P2P 网络**——即"迅雷用户客户端之间 +
迅雷服务器(超级种子/调度)参与分发"的那一层加速通道(区别于标准 BT 互连)。

- 若能: 给出协议文档 + 最小复现(连上并拉到数据)
- 若不能: 给出技术/法律壁垒的证据链

## 2. 背景 (先读这些, 避免重复劳动)

- 文件格式部分**已全部完成并经验证**: 仓库 `tools/xunlei-migrate/samples/` 真实样本
  (audio-books-cjk, infohash C5AA149AE0776344A270EAFEE49FDADB43FF6097, 2263 pieces @131072)
  + `validate_xunlei_sample.py` V1-V8 全绿 + `spec_pending_validation.md` (A 级)。
  结论: .xlbt.cfg 是任务元数据(无哈希/位图), .bt.xltd 是文件位置镜像,
  标准 BT piece 哈希可 SHA1 推导 → 数据可迁移。这部分**不需要你再做**。
- 旧评估: `docs/research/xunlei/DECISIONS.md` 路径 B(原生接入迅雷网络)曾评
  6-18 个月 + 法律风险高 + 不推荐。**现在重新立项**, 原因: 用户明确想要这层加速;
  用户机器上有真实迅雷环境可以抓包验证, 成本大幅下降。
- 旧协议线索: `docs/research/xunlei/xunlei_research_complete.md` (合集, 含
  analyze_protocols.py / analyze_proto_classes.py / analyze_tcp_xudt.py /
  all_proto_constants.txt / PHUB 常量 等)。

## 3. 已有线索 (逆向起点, 来自 DownloadSDK.dll 4.7MB, v25.0.90.1592)

### 3.1 协议类簇: 25 个 XBTPackage* 类 (RTTI 提取)

标准 BT 部分 (BEP 编号):
- Handshake / KeepAlive / Choke / UnChoke / Interest / NotInterest (BEP-3)
- Have / HaveAll / HaveNone / BitField (BEP-3)
- Request / RejectRequest / Cancel (BEP-3, 6)
- ExtHandshake (BEP-10) / Metadata (BEP-9) / PEX (BEP-11)
- AllowedFast (BEP-6) / MSE (BEP-8) / Port (BEP-5)

迅雷自有 (重点):
- **PunchingHole** — NAT 打洞消息, 载荷格式未解
- **SuggestPiece** — 建议 piece 消息, 载荷格式未解

### 3.2 会话/网络类

- XBTInputChannelSession / XBTOutputChannelSession — BT peer 会话
- DHTDelegation — DHT (是否标准需确认)
- TCP/uDT — 自研可靠 UDP? 已有分析脚本 (analyze_tcp_xudt.py)
- DCDNResource / ConstSizeDataPieceManager — P2SP / 块级调度资源

### 3.3 已知公开资料 (参考, 不是结论)

- ThunderPlatform 协议逆向 + Wireshark 插件 + 加密握手解析:
  https://my.oschina.net/emacs_7995965/blog/19395552
- Go 实现迅雷极速通道协议 XLP v3.2 (源码级): https://datasea.cn/go0320541706.html
- Xunlei-Fastdick (快鸟/极速通道 Python): https://github.com/fffonion/Xunlei-Fastdick
- 学术: "Unreeling Xunlei Kankan" 混合 CDN-P2P 流媒体架构 (IEEE TMM 2014)
- 学术: PAM 2012 "Unreeling Xunlei" 迅雷 P2P 系统测量: https://dlnext.acm.org/doi/10.1007/978-3-642-28537-0_23
- 老 CLI xunlei-lixian (CID/DCID 算法): https://github.com/iambus/xunlei-lixian

## 4. 工作环境 (用户机器)

- Windows 11, 已装迅雷 (C:\Program Files\Thunder Network\Thunder\)
- 正在下载的真实任务: audio-books-cjk (~83%), 可抓包
- 可用工具: Wireshark/npcap, capstone, x64dbg, python3 + libtorrent, Go
- 可研究的样本 DLL: 迅雷安装目录下的 DownloadSDK.dll 等 (4.7MB, 63k 字符串)

## 5. 分阶段任务 (P0 → P3)

### P0 摸底 — 先回答"这层网络长什么样" (1-2 天)
1. 抓包: 用迅雷下载一个标准 BT/磁力任务 (可用 audio-books-cjk),
   Wireshark 按连接分类: 标准 BT 流量 (pstr="BitTorrent protocol") /
   私有握手 / HTTP 调度 / DHT (UDP 6881) / 其他
2. 确定私有部分握手形态:
   - 迅雷 peer 的标准握手 pstr 是什么? 扩展握手 (BEP-10) 里 `<extid>ut_metadata/ut_pex/lt_donthave` 之外,
     还有哪些**自研扩展 id**?
   - PunchingHole / SuggestPiece 的消息 id 号 (标准约定之外的空位) 与载荷
3. 迅雷 DHT 是否标准 (节点 id 派生、bootstrap 节点列表、announce_peer 行为)
4. 服务器角色清单: HTTP/UDP tracker、P2SP 调度服务器 (thunder:// 内嵌地址)、
   超级种子 (长期在线 peer)、以及任何非标准端口上的连接

### P1 协议文档化 — 写出能复现的包格式 (2-4 天)
5. 私有流量的加密判定 (决定性): 是 MSE (BEP-8 rc4) / TLS / 自研 XOR / 明文?
   - MSE 若为标准 → 复用 libtorrent 的 MSE 即可进入加密层
   - 自研 → 从 DownloadSDK.dll 反汇编密码学函数 (找 Rijndael/RC4/自研混淆)
6. 完整记录: 握手字节流、扩展消息、PunchingHole 载荷、SuggestPiece 载荷、
   PEX 是否私有变体、KeepAlive 间隔等
7. 服务器调度协议: P2SP 调度 URL/参数/响应格式, 返回的 peer 列表能否直接用
   (ip:port 直连?)

### P2 最小复现 — 证明"第三方可进入" (3-7 天)
8. 用 Python/Go 实现: 标准 BT 握手 + 扩展握手 → 伪装/作为普通 peer 加入
   迅雷网络的同一任务 (同一 infohash)
9. 关键尝试: 从迅雷 peer / 迅雷服务器拉到 ≥1 个 piece 数据 (SHA1 验证)
10. 若该网络有开放 guest 通道 (无账号鉴权) → 记录接入条件;
    若必须账号鉴权 → 给出鉴权依赖的证据 (哪些字段绑定账号/会话)
11. 复现代码与抓包 pcap 一起作为产物

### P3 集成评估 — 回答"值不值得做" (1 天)
12. libtorrent (C++/Rust) 侧接入路径评估:
    - 方案 a: libtorrent plugin 实现自定义 peer 连接插件 (BT plugin API)
    - 方案 b: 独立下载引擎 (自写) 只处理"迅雷私有通道"
    - 各自工作量 / 稳定性 / 与调度器的接口
13. 给出最终结论: 可接入 (路径+工作量) / 不可接入 (证据)

## 6. 验收标准 (全部可举证)

- [ ] 协议文档: 握手 + 扩展 + 私有消息 + 服务器接口 (带十六进制示例)
- [ ] pcap 抓包证据 + 解析结果 (Wireshark 或自写解析器)
- [ ] 最小复现代码: 能连上迅雷网络同一任务并验证拉到数据 (SHA1)
- [ ] 鉴权结论: 有/无账号绑定, 证据在哪
- [ ] P3 集成评估表 + 明确路线建议

## 7. 约束 (必须遵守)

- 个人研究用途: 只连接用户自己的任务相关节点, 不做耗尽性/放大攻击
- 不碰会员鉴权机制 (离线下载 / 高速通道 / 云加速): 只研究开放 P2P 通道
- 输出协议文档化(白盒描述), 不发布破解绕过类工具
- 法律风险自评估: 逆向协议文档 + 个人使用在国内属灰色地带, 结论里标注风险等级

## 8. 交付格式

- 单篇 markdown 报告: `xunlei_p2p_network_recon.md` (含全部证据链接/片段)
- 代码 + pcap + 反汇编脚本放入独立目录, 报告内附路径
- 最终三句话结论: 能不能 / 怎么接 / 多少工作量