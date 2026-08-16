# 多引擎智能调度下载器 — 设计文档（Design v0.6 · 决策收口 + 独立评审 12 条已处置）

> **状态：2026-08-16。23 项决策已拍板；v0.6 吸收独立评审 12 条（含 1 处事实修正）。架构冻结，进入 M0。**
> 配套文件：TDD 实施计划 `2026-08-16-smart-downloader-m0-m7-tdd.md`。

---

## 0. 项目背景与开发需求（供独立评审，无需对话上下文）

### 0.1 这是什么

**个人自用、代码可分享**的智能下载器（Rust 调度层 + C++/libtorrent 薄内核）：

- 统一接入 magnet / .torrent / http(s) / ftp / thunder://（解码）
- 能力模型驱动路由：BT 热门 → libtorrent 底座；HTTP/FTP → reqwest 传输 + 自研调度层；冷门 BT → 云兜底（debrid/115 Provider，默认关，**不自动烧配额**）
- 统一任务/文件模型、去重、统一输出、元数据分离（session 目录）
- 生态健康：反吸血记录告警（不强制 ban）、上传下载比统计（非完成条件）

### 0.2 关键约束（含 v0.6 事实修正）

| 约束 | 结论（v0.6 修正后） |
| :--- | :--- |
| BT 引擎选型 | **libtorrent（BSD-3）**。理由排序：① 15+ 年生产验证；② 协议完备（Web Seed BEP-19/17、peer 级控制、DHT、uTP 全内建且大规模测试）；③ BSD 许可宽松。~~"librqbit 是 GPL-3 不能嵌"~~ **已修正（D30）：librqbit 实际是 Apache-2.0**（2026-08-16 核查 rqbit 官方 LICENSE），许可不构成否决理由；rqbit 仍因 BEP-19 支持有限、无逐 peer 封禁、较年轻而落选 |
| 集成形态 | 进程/HTTP 集成拿不到 piece 级与 peer 级控制 → **C ABI 薄内核**。绑定工具（手写 C ABI vs cxx）由 **M0 spike 双写对比后冻结**（D28，评审问题 1） |
| 工具链 | cbindgen 是 Rust→C 工具，方向不符 → **手写 `lt.h` 契约 + bindgen 生成 Rust 侧**（spike 后确认） |
| HTTP 引擎 | **reqwest 做传输，自研的是调度层**（分块/换源/.part/重试/镜像）——不重写 HTTP 协议（D29，评审问题 2 澄清） |
| 云兜底 | 迅雷云盘不做（账号+会员+配额+签名盐脆弱依赖）；死种兜底用 debrid/115 官方 API |
| 迅雷 BT | 闭源无 API 不集成；P2SP"服务器多源"已有等价物（Web Seed + 镜像 + Provider）→ 仅生态认知（反吸血规则） |
| BT↔云切换 | 云端给整文件、无法 piece 级接管 BT 半成品 → 并行获取 + FallbackPolicy，**BT 半成品绝不自动删除** |
| 反吸血 | v1 只记录告警；ban 与行为检测 v2（FFI 富字段已就位） |

### 0.3 评审阅读顺序

§1 → §3 → §9（所有权，核心）→ §10（状态机）→ §8（FFI）→ §12/§13/§14 → §20（决策回填）→ TDD 计划。

---

## 1. 决策记录

### 1.0 已定决策（D1–D27，含 v0.5）

D1 定位：个人自用、代码可分享｜D2 BT=libtorrent 薄内核｜D3 反吸血 v1 记录（FFI 富 peer）｜D4 快照轮询+alert 扁平化+alerts_dropped｜D5 Provider 含运行态｜D6 数据所有权：引擎只有传输权｜D7 三阶段评估｜D8 任务 ID 三层分离｜D9 canonical_id 身份模型｜D10 Provider 与传输分离｜D11 HTTP 静态分块 64MB/2–8 连接/运行时换源/ETag 优先｜D12 FTP 并入 httpdl（feature）｜D13 FFI 内存模型｜D14 手写 lt.h + bindgen（M0 spike 复核）｜D15 任务级状态机+文件级进度｜D16 resume 异步 alert 流、Rust 唯一恢复所有者｜D17 ContentIdentity｜D18 v1 启发式路由、v2 多因素 score｜D19 Windows pause→torrent_paused_alert 同步点｜D20 HTTP 自研（=reqwest 传输+自研调度，D29 澄清）｜D21 CLI+WS｜D22 本机单用户｜D23 FallbackPolicy 0.5/禁双份/KeepLarger/重下≤2｜D24 B 组 12 项（完成即停状态分离、并发 BT3+HTTP8+Provider2 FIFO、末段路径+大小对齐、ETag+Length 校验重下1次降级、v1 全选、resume 4 时机、Failed 保留、metadata 超时手动兜底、预检、跨盘、凭证明文、WS 9 事件、单实例锁、BT 网络参数）｜D25 C 组参数｜D26 D 组工程（TOML 冷加载、CLI 命令集、**v1 最小 CI（D35 修正）**、本地 BT seed 基建）｜D27 迅雷云盘不做、thunder:// 解码保留

### 1.1 v0.6 独立评审处置表（12 条）

| 评审点 | 优先级 | 处置 | 落点 |
| :--- | :--- | :--- | :--- |
| 1. cxx vs 手写 C ABI 未论证 | P0 | **接受**：M0 spike 双写 200 行（`lt_session_create`/`lt_pop_alerts`）对比后冻结 D14 | §8.1 / TDD M0 |
| 2. 自研 HTTP 投入产出比存疑 | P0 | **澄清**：HttpEngine = reqwest 传输 + 自研调度层（分块/换源/.part/重试），**不重写 HTTP 协议**；M4 范围同步收紧 | D29 / §14 / TDD M4 |
| 3. librqbit 许可证事实 | P0 | **修正**：librqbit = Apache-2.0（官方 LICENSE 核查）；选型理由改为成熟度/协议完备性/peer 控制 | D30 / §0.2 |
| 4. alert 扁平化工作量被低估 | P1 | **接受**：v1 alert 预算 **≤12 种**，明确清单 | §8 / TDD M1 |
| 5. PausingAwait 不应是通用状态 | P1 | **接受**：改为内部同步点（平台相关等待），对外 Stalled→Paused/FallbackProvider | §10 / TDD M2 |
| 6. PieceHashed 属 v2 过度设计 | P1 | **接受**（YAGNI）：v1 只留 InfoHash/SingleFile；v2 加 PieceHashed 走 schema version | §7 / TDD M2·M3 |
| 7. canonical_id token 参数边界 | P2 | **接受**：token 参数黑名单（见 §7） | §7 / TDD M2 |
| 8. 磁盘预检对小文件严苛 | P3 | **接受**：分段公式 `max(total×1.1, total+min(500MB,total))` | §12 / TDD M3 |
| 9. WS 背压缺细节 | P3 | **接受**：事件带 monotonic seq，跳号 → 客户端拉快照补齐 | §12.4 / TDD M6 |
| 10. 凭证明文+权限位 | P3 | **接受**：config.toml 启动 chmod 0600（Unix）/ ACL（Windows） | §12 / TDD M3 |
| 11. thunder:// 解码规则未定义 | P3 | **接受**：base64(`AA`+url+`ZZ`) 剥壳，`core/src/source_parse/thunder.rs` | §7.1 / TDD M2 |
| 12. v1 无 CI 风险 | P2 | **接受**：最小 CI（fmt+clippy+纯 Rust crates 测试；BT 集成测试 CI 外后置） | D35 / TDD 收尾 |

### 1.2 v0.6 新增决策

- **D28**：FFI 绑定工具 M0 spike 双写（手写 C ABI vs cxx）后冻结；默认倾向手写 C ABI（契约定死、alert 扁平化本就在 C++ 侧、避免 codegen 依赖），以 spike 实测为准
- **D29**：HttpEngine = **reqwest 传输 + 自研调度层**；"自研"指分块/换源/.part/重试/镜像，不重写 HTTP 协议
- **D30**：librqbit 许可证 = **Apache-2.0**（事实修正）；选型理由 = 成熟度 + BEP-19/peer 控制 + 协议完备性
- **D31**：FFI alert v1 预算 ≤12 种（§8 清单）
- **D32**：PausingAwait 内部化（非 TaskState 枚举值）
- **D33**：ContentIdentity v1 两态（InfoHash/SingleFile）；PieceHashed v2 + schema version 升级
- **D34**：canonical_id token 参数黑名单（§7）
- **D35**：最小 CI：`fmt --check` + `clippy -D warnings` + `cargo test -p core -p httpdl -p provider -p daemon`（纯 Rust，无 libtorrent）；BT 集成测试（需 libtorrent）后置独立 job/本地
- **D36**：杂项细节（§12）：磁盘预检分段公式、WS seq/resync、config 权限、thunder 解码规则

---

## 2. 目标与非目标

### 目标（v1）
1. 统一接入 magnet/.torrent/http(s)/ftp/thunder://；任务去重
2. 能力路由 + v1 启发式路由（heat 阈值）
3. BT：Web Seed / add_peer / tracker 池 / 顺序下载 / 反吸血记录（富 peer）
4. HTTP（reqwest+调度层）：静态分块、Range 探测、头/认证/代理/TLS、.part 续传、ETag 优先、运行时换源、重试、镜像、校验
5. FTP 最小引擎（feature-gated）
6. 云兜底：Provider（运行态/配额）→ 直链 → HttpEngine；受 FallbackPolicy；不自动烧配额
7. 统一输出（BT 直写 vs .part 诚实化）；resume Rust 承载；单实例锁
8. WebSocket（9 类事件、seq+背压）+ CLI；磁盘预检；最小 CI

### 非目标（v1）
- piece 级多来源协同（v2）
- 强制反吸血（v2）
- 长效种子 / 迅雷 P2SP / 迅雷云盘 / 迅雷本地客户端
- GUI、Digest、SFTP、BT 选择性下载（v2）、HTTP/3 精细控制

---

## 3. 总体架构

```
                            Smart Scheduler (Rust)
                                    │
                  DownloadTask（身份 + 所有权边界，§7/§9）
                                    │
                          EngineRegistry（能力路由 §4）
              ┌───────────────┬─────┴────────┬────────────────┐
        ┌─────▼─────┐   ┌─────▼──────┐   ┌───▼──────────┐   │
        │BtEngine   │   │HttpEngine  │   │(ftp 并入)    │   │
        │libtorrent │   │reqwest传输+│   │protocol::ftp │   │
        │(C ABI)    │   │自研调度层   │   │              │   │
        └─────┬─────┘   └─────┬──────┘   └──────────────┘   │
              │               │                             │
       RemoteProvider（≤2 并发，默认关，只远程获取）             │
       115/debrid/mock → resolve() 直链 → HttpEngine          │
                              ▼                             │
              Unified Output / Session（Rust 承载恢复，§12）
```

**混合事件模型（D4）**：`lt_status` 快照轮询（1s）；离散事件 `lt_pop_alerts`（500ms–1s，扁平化拷贝）；`alerts_dropped>0` → 对目标 ih 重拉快照补缺。

**并发配额（D24）**：BT ≤3、HTTP/FTP ≤8、Provider ≤2；超出进 `Queued`（FIFO）。

---

## 4. 统一能力模型

```rust
pub enum DownloadSource {
    Magnet(String), TorrentFile(Vec<u8>),
    Http { url: String, headers: Vec<(String,String)>, auth: Option<Auth> },
    Ftp { url: String, user: String, pass: String },
    Thunder(String),   // 解码为 Http（§7.1）
    Ed2k(String),      // v1 不支持 → Failed
}
pub enum Auth { Basic(String,String), Bearer(String) }   // Digest v2

pub enum Capability {
    Magnet, TorrentFile, Peer, Tracker, Dht, WebSeed,
    PieceRead, PeerBan, Sequential, Stream,
    Http, Https, Range, MultiConnection, Mirror, UrlRefresh,
    Ftp, FtpResume, OfflineCache,
}

#[async_trait]
pub trait DownloadEngine: Send + Sync {
    fn id(&self) -> &str;  fn kind(&self) -> EngineKind;  fn capabilities(&self) -> Vec<Capability>;
    async fn add(&self, task: &DownloadTask) -> Result<EngineTaskId>;
    async fn pause(&self, id: &EngineTaskId) -> Result<()>;
    async fn resume(&self, id: &EngineTaskId) -> Result<()>;
    async fn status(&self, id: &EngineTaskId) -> Result<EngineStatus>;
    async fn remove(&self, id: &EngineTaskId, delete_data: bool) -> Result<()>;
    async fn peers(&self, id: &EngineTaskId) -> Result<Vec<PeerInfo>>;
    async fn update_sources(&self, id: &EngineTaskId, urls: Vec<String>) -> Result<()>;
    async fn add_url_seed(&self, id: &EngineTaskId, url: &str) -> Result<()>;   // Bt
    async fn ban_peer(&self, id: &EngineTaskId, peer: SocketAddr) -> Result<()>;// Bt v2
    async fn read_piece(&self, id: &EngineTaskId, idx: u32) -> Result<Vec<u8>>; // Bt v2
}
pub struct PeerInfo { ip, port, peer_id, client, progress_ppm, down_rate, up_rate,
    total_download, total_upload, last_active_sec, flags }
pub struct EngineStatus { state, metadata_received, files: Vec<FileProgress>,
    total_done, total, down_rate, up_rate, num_peers, num_seeds, error }
```

**路由矩阵**：Magnet/Torrent→BtEngine；Http→HttpEngine；Ftp→FtpEngine；Thunder(解码)→HttpEngine；Ed2k→Failed。

**去重**：入队前查 `tasks_by_canonical`（§7），重复 → `DuplicateRejected`。

---

## 5. 目录结构

```
smart-downloader/
├─ Cargo.toml                # workspace（core, btcore, httpdl, provider, daemon）
├─ .github/workflows/ci.yml  # 最小 CI（D35）
├─ crates/
│  ├─ core/          # types / source_parse(thunder) / task / ownership / registry / state_machine / heat / router / dedup / health / output / session
│  ├─ btcore/        # ffi.rs(unsafe) / alerts.rs / resume.rs / engine.rs；build.rs bindgen 校验
│  ├─ httpdl/        # engine / range / static_split / resume / retry / mirror / sources / verify / protocol/ftp.rs
│  ├─ provider/      # RemoteProvider + ProviderRuntime + mock + (115/debrid, 默认关)
│  └─ daemon/        # monitor_loop / ws(seq+背压) / cli
├─ ffi/lt.h          # ★ 手写 C ABI 契约（入库锁定；非 cbindgen 产物）
├─ ffi/src/lt_kernel.cpp
├─ docs/superpowers/plans/
└─ tests/integration/       # BT seeder / HTTP server / FTP server
```

---

## 6. 吸收清单

| 吸收自 | 能力 | 落点 | v1/v2 |
| :--- | :--- | :--- | :--- |
| aria2 | HTTP 多源 | BtEngine Web Seed；HttpEngine mirror | v1 |
| qBittorrent-EE | Tracker 池/黑名单规则 | add_tracker / health 规则表 | v1 |
| DHT 爬虫 | peer 注入 | add_peer | v1 |
| rqbit | 边下边播 | 顺序下载 + 私有流服务 | v2 |
| qBittorrent | 元数据分离 | resume Rust 承载 | v1 |
| PeerBanHelper | 行为检测 | v2（富字段已就位） | v2 |
| 115/debrid | 冷门兜底 | RemoteProvider | v1(mock/debrid) |

**吸收不进**：长效种子；迅雷 P2SP；迅雷本地客户端；迅雷云盘。

---

## 7. 任务与文件模型

```rust
pub struct DownloadTask {
    pub id: TaskId, pub canonical_id: CanonicalId, pub source: DownloadSource,
    pub identity: ContentIdentity, pub dest_root: PathBuf,
    pub files: Vec<TaskFile>, pub acquisitions: Vec<Acquisition>,
    pub aggregate: ProgressAggregate, pub state: TaskState, pub retry: RetryState,
    pub created_at: Instant, pub metadata: TaskMetadata,
}
pub struct TaskFile { rel_path, size, done, state: FileState, source_urls,
    identity: Option<ContentIdentity>, etag, engine: EngineKind }

pub struct CanonicalId { kind, identity: String, validator: Option<String> }
// BT: btih(40hex) ｜ TorrentFile: SHA256(bytes) ｜ FTP: URL+size+mtime
// HTTP: identity=normalized URL（去 fragment，保留 query）
//   ★ token 参数黑名单（D34）：`token|sig|signature|expires|auth|X-Amz-*|X-Goog-*|X-Tencent-*|X-QiNiu-*`
//   命中黑名单的 query 从 normalize 中剔除；其余 query 参与 identity
//   带 token 的 URL 默认不自动去重，仅当 validator（size/etag）与既有任务一致才认重

pub enum ContentIdentity {   // D33：v1 两态
    InfoHash([u8; 20]),
    SingleFile { size: u64, etag: Option<String>, sha256: Option<String> },
    // PieceHashed 属 v2（PieceCoordinator 用），v2 通过 session schema version 升级加入
}
```

### 7.1 thunder:// 解码规则（D36）

```
thunder:// 内容 = base64("AA" + 真实URL + "ZZ")
解码：去前缀 "thunder://" → base64 decode → 剥头 "AA" 尾 "ZZ" → 得真实 URL
实现：crates/core/src/source_parse/thunder.rs（≤30 行），M2 单测覆盖
```

**文件对齐（D24）**：BT 种子文件与云 resolve 文件按"相对路径末段 + 大小"对齐；无法对齐时云版本按自身 rel_path 落盘。

---

## 8. FFI 接口契约（v0.6：含 cxx spike 决策点 + alert 预算）

### 8.1 绑定工具（D28，M0 spike 决出）

- M0 用 **手写 C ABI** 与 **cxx** 各写 ~200 行最小内核（`lt_session_create` / `lt_pop_alerts` / `lt_status`），对比：构建复杂度、内存契约维护、异常/回调处理、alert 扁平化工作量
- 决策倾向（以 spike 实测为准）：**手写 C ABI + bindgen**——契约定死可复用于其他语言/工具、alert 扁平化本就在 C++ 侧做一层、无 codegen 构建依赖；cxx 若证明能显著降低 lt_kernel.cpp 复杂度则改选

### 8.2 内存与所有权规则（D13）

- 输出缓冲 Rust 预分配 + capacity，C++ 写入 ≤cap；`LT_ERR_BUFFER_TOO_SMALL` → 扩容重试
- 无 new[]/静态缓冲/所有权转移；字符串定长数组，Rust 立即拷贝
- **alert 由 C++ 扁平化为值结构复制进 Rust 缓冲**（不持有 libtorrent 内部指针）

### 8.3 函数清单（全量 ~28）

```c
typedef struct lt_session lt_session;
typedef enum { LT_OK=0, LT_ERR_ARG, LT_ERR_ENGINE, LT_ERR_IO, LT_ERR_NOT_FOUND,
               LT_ERR_BUFFER_TOO_SMALL } lt_err;
lt_err lt_session_new(const char* save_path, const char* session_id, lt_session** out);
void   lt_session_free(lt_session* s);
lt_err lt_err_str(lt_session* s, char* buf, size_t cap, size_t* out_len);

lt_err lt_add_magnet(lt_session* s, const char* magnet, const char** web_seeds, char* ih_out);
lt_err lt_add_torrent_file(lt_session* s, const uint8_t* meta, size_t len, const char** web_seeds, char* ih_out);
lt_err lt_add_torrent_resume(lt_session* s, const uint8_t* resume_data, size_t len, const char** web_seeds, char* ih_out);
lt_err lt_pause(lt_session* s, const char* ih);   /* 完成以 torrent_paused_alert 为同步点（D19/D32） */
lt_err lt_resume(lt_session* s, const char* ih);
lt_err lt_remove(lt_session* s, const char* ih, int delete_data);

typedef struct { int state; float progress; int64_t downloaded,total,down_rate,up_rate;
                 int num_peers,num_seeds; int metadata_received; } lt_torrent_status;
lt_err lt_status(lt_session* s, const char* ih, lt_torrent_status* out);
lt_err lt_piece_count(lt_session* s, const char* ih, int* out);
lt_err lt_bitfield(lt_session* s, const char* ih, uint8_t* buf, size_t cap, size_t* out_len);
lt_err lt_file_count(lt_session* s, const char* ih, int* out);
lt_err lt_file_progress(lt_session* s, const char* ih, int64_t* done_arr, int64_t* size_arr, int n);

typedef struct { char ip[64]; uint16_t port; char peer_id[64]; char client[128];
                 uint32_t progress_ppm; int64_t down_rate,up_rate;
                 int64_t total_download,total_upload,last_active_sec;
                 uint32_t flags; /* seed/uploader/interesting/choked/snubbed/connecting/local/utp */ } lt_peer;
lt_err lt_peers(lt_session* s, const char* ih, lt_peer* buf, size_t cap, size_t* out_count);

typedef enum { LT_ALERT_TRACKER=1, LT_ALERT_PEER=2, LT_ALERT_ERROR=4, LT_ALERT_METADATA=8,
               LT_ALERT_STATE=16, LT_ALERT_RESUME=32, LT_ALERT_PIECE=64 } lt_alert_mask;
lt_err lt_set_alert_mask(lt_session* s, const char* ih, uint32_t mask);
typedef struct { int kind; char ih[41]; char msg[512]; int64_t at; int resume_ready; } lt_alert;
lt_err lt_pop_alerts(lt_session* s, lt_alert* buf, size_t cap, size_t* out_count);
lt_err lt_alerts_dropped(lt_session* s, uint32_t* out);

lt_err lt_request_save_resume(lt_session* s, const char* ih);
lt_err lt_take_resume_data(lt_session* s, const char* ih, uint8_t* buf, size_t cap, size_t* out_len);

lt_err lt_ban_peer(lt_session* s, const char* ih, const char* ip, uint16_t port); /* v2 */
lt_err lt_add_peer(lt_session* s, const char* ih, const char* ip, uint16_t port);
lt_err lt_add_url_seed(lt_session* s, const char* ih, const char* url);
lt_err lt_add_tracker(lt_session* s, const char* ih, const char* url);
lt_err lt_set_sequential(lt_session* s, const char* ih, int on);
lt_err lt_set_limits(lt_session* s, const char* ih, int64_t down_limit, int64_t up_limit);
lt_err lt_read_piece(lt_session* s, const char* ih, int idx, uint8_t* buf, size_t buflen, size_t* out_len); /* v2 */
```

### 8.4 v1 alert 预算（D31，≤12 种扁平化）

| C++ 侧 case | 对应 libtorrent alert | 用途 |
| :--- | :--- | :--- |
| METADATA | metadata_received | F2 三阶段推进 |
| STATE_FINISHED | torrent_finished | Completed → Stopped（做种停止，§10.1） |
| STATE_PAUSED | torrent_paused | D19/D32 同步点 |
| STATE_ERROR | torrent_error | 失败重试 |
| RESUME_SAVED | save_resume_data | D16 落盘 |
| RESUME_FAILED | save_resume_data_failed | 重试 |
| TRACKER | tracker_announce/tracker_error | 事件 |
| PEER | peer_connected/peer_disconnected | 事件 |
| PIECE | piece_finished | 进度事件（可选，低优先） |
| DHT | dht_* | 评估辅助（可选） |

> 其余 ~70 种 libtorrent alert 在 v1 **不扁平化**（C++ 侧丢弃并计数），避免 M1 失控。

### 8.5 alert 字段级 schema（D31 终稿，M0.3 闸门产出）

`lt_alert` 为统一值结构 `{ kind: int, ih[41], msg[512], at: int64, resume_ready: int }`。各 kind 的字段填充规则：

| kind | 触发源（libtorrent alert） | ih | msg 填充 | resume_ready |
| :--- | :--- | :--- | :--- | :--- |
| METADATA | `metadata_received_alert` | 对应 ih | "metadata received" | 0 |
| STATE·FINISHED | `torrent_finished_alert` / state_changed→finished_seeding | 对应 ih | "finished" | 0 |
| STATE·PAUSED | `torrent_paused_alert`（D19 同步点） | 对应 ih | "paused" | 0 |
| STATE·ERROR | `torrent_error_alert` | 对应 ih | error.message() | 0 |
| RESUME·SAVED | `save_resume_data_alert` | 对应 ih | "resume ready" | **1**（随后 Rust 调 lt_take_resume_data） |
| RESUME·FAILED | `save_resume_data_failed_alert` | 对应 ih | 失败原因 | 0 |
| TRACKER | `tracker_error_alert` / `tracker_announce_alert` | 对应 ih | tracker url + 状态/错误 | 0 |
| PEER | `peer_connected_alert` / `peer_disconnected_alert` | 对应 ih | ip:port + 事件 | 0 |
| PIECE | `piece_finished_alert`（可选，低优先） | 对应 ih | piece 索引 | 0 |
| DHT | `dht_*`（可选） | "" | dht 统计 | 0 |

Rust 侧 `AlertEvent` 枚举与之 1:1，serde 序列化为 WS 事件（§12.4）。字段增删 = 改 lt.h + cpp case + Rust 枚举（bindgen 重跑），改动面已封顶。

---

## 9. 所有权边界模型（核心）

> 四条边界：谁拥有数据 / 谁拥有文件 / 谁负责恢复 / 谁可以替换来源。

| 边界 | 定论 |
| :--- | :--- |
| 数据所有权 | Task 拥有 files[]；Acquisition 只是候选数据集；引擎只有传输权，**无权删数据** |
| 文件所有权 | 输出层拥有最终文件；BT 直写（resume 保护）、HTTP/FTP/云经 .part；只有 Output 层可 rename/replace |
| 恢复所有权 | Rust 唯一恢复所有者（state.json / resume.bencode / .part） |
| 来源替换权 | 仅 Router/用户换来源；引擎接受 update_sources 无权自行决策 |

```rust
pub struct Acquisition { kind: AcqKind, engine_id, engine_task_id, state, done, total, started_at }
pub struct FallbackPolicy {
    pub bt_ratio_to_continue: f64,     // 0.5
    pub allow_parallel_disk: bool,     // false（禁止双份占盘）
    pub on_both_partial: KeepLarger,
    pub max_provider_redownloads: u32, // 2
}
```

**兜底触发**：仅 (a) 热度 <0.3 且策略允许；(b) BT stall（30s/<1MB）且 BT 进度 <50%。**metadata 超时绝不自动触发（Q-B9，手动 `fallback` 命令）**。半成品绝不自动删除。

---

## 10. 状态机与路由（v0.6：PausingAwait 内部化）

### 状态图

```
Queued ──> Evaluating
   │         ├─ MetadataPending（磁力/种子；metadata 前 peer 数不参与冷门判定）
   │         ├─ PeerDiscovery（15s 观察窗，2 次采样）
   │         ├─ HeatEvaluating（评分 → 路由）
   │         └──► Downloading(Bt) / FallbackProvider（按 FallbackPolicy）
Downloading(Bt) ──30s无进展(且<50%)──> Stalled{Bt}
   │                                   └─（内部同步点：发 lt_pause → 等 torrent_paused_alert，D32）
   │                                        → Paused / FallbackProvider
Downloading(Http/Ftp) ──> Completed ──> Stopped（默认）/ Seeding（配置开启）
FallbackProvider → Ready → Transferring(HttpEngine) → Completed
Transferring：直链失效 → update_sources(≤3) → resubmit(≤2)
```

**TaskState 枚举（对外）**：`Queued | Evaluating{phase} | Downloading{engine} | Paused | FallbackProvider | Transferring | Completed | Stopped | Seeding | Failed`。
**PausingAwait 不是枚举值（D32）**：是 `Stalled → (Paused|FallbackProvider)` 转换内部、平台相关的等待点（Windows 等 torrent_paused_alert 保证文件句柄释放；Linux/macOS 可直接快照）。

### 完成与做种分离（D24，写死）

- **Completed = 文件已满足下载完成条件**（全部就位且校验通过）
- **Seeding = Completed 后继续运行 BT 上传**（仅配置开启）
- **上传比 ≥0.5 是统计指标，不是 Completed 必要条件**
- 实现（§10.1）：监听 torrent_finished → `lt_pause`；不依赖 seed_ratio（版本 bug 规避）

### 转换表（关键行）

| from | to | 条件 |
| :--- | :--- | :--- |
| Queued | Evaluating | 并发配额有空位（BT≤3/HTTP·FTP≤8/Provider≤2），否则留 Queued（FIFO） |
| Queued | （拒绝） | canonical_id 重复 |
| Evaluating·MetadataPending | PeerDiscovery | metadata_received==1；**60s 超时 → 保持 BT + FallbackAvailable 标志（手动兜底），不自动烧配额** |
| Evaluating·HeatEvaluating | Downloading(Bt) | 热度 ≥0.3 |
| Evaluating·HeatEvaluating | FallbackProvider | 热度 <0.3 且策略允许 |
| Downloading(Bt) | Stalled | 30s 增量 <1MB 或错误（重试后） |
| Stalled | Paused / FallbackProvider | 内部等 torrent_paused_alert 后：BT<50% → 兜底（串行）；≥50% → Paused（可恢复） |
| Downloading/Transferring | Completed | 全文件完成且校验通过 |
| Completed | Stopped | 默认；Seeding 仅配置开启 |
| * | Failed | 重试超上限 / 无可用源 / Ed2k |

### 10.1 做种停止注记（M1 注意）

libtorrent `seed_ratio/seed_time` 为 session 级且有版本 bug → 监听 `torrent_finished`（state_changed alert）→ 立即 `lt_pause(ih)`，Rust 置 `Completed → Stopped`。持续做种（配置开启）不做 pause。

### 10.2 v1 启发式路由

```
热度 = clamp(avg_peers/50,0,1)*0.7 + clamp(avg_seeds/10,0,1)*0.3
热 ≥0.7 → BT；0.3–0.7 → BT+30s 无进展→兜底；<0.3 → 直接兜底（受 FallbackPolicy）
v2：score = availability × expected_speed × reliability − cost − risk
```

---

## 11. 生态健康 v1

- 每 30s 对活动 BT 任务 `lt_peers`；`client`+`peer_id` 双字段 + flags/progress_ppm/累计上传/活跃时间 记录
- 黑名单规则抄 qBittorrent-EE（`-XL`/`XL`/`-SD`/`-BN`/`-DT`）→ `HealthEvent::LeechDetected`，不 ban
- 上传下载比：累计字节 <0.5 → `HealthEvent::RatioLow`（仅统计告警）
- v2：假进度/装死/反复重连/伪装检测（字段已就位）

---

## 12. 输出、续传、会话与事件细节（含 D36）

```
~/Downloads/smart-dl/                    # 输出：BT 直写；HTTP/FTP/云 经 .part rename 落位
~/.config/smart-dl/sessions/<task_uuid>/ # state.json / resume.bencode / .part/ / log/
~/.config/smart-dl/config.toml           # TOML 冷加载；启动 chmod 0600(Unix)/ACL(Windows)（D36）
~/.config/smart-dl/lock                  # 单实例锁
```

- **恢复（D16）**：保存 = `lt_request_save_resume` → alert → `lt_take_resume_data` → Rust 落盘；时机 = 暂停/完成/退出 + 每 10 分钟。恢复 = state.json → resume.bencode → `lt_add_torrent_resume`
- **磁盘预检（D36 分段公式）**：`required = max(total×1.1, total + min(500MB, total))`；剩余 < required → 拒绝入队
- **跨盘（D24）**：允许；rename 失败 → copy + 删源 + 警告
- **Windows（D19/D32）**：pause → torrent_paused_alert → 文件操作；rename 共享冲突退避 ≤5 → 提示
- **单实例锁（D24）**：lock 已存在 → 向现有实例转发任务后退出
- **WS 事件协议（D36）**：9 类事件（TaskCreated/StateChanged/Progress/Speed/HealthEvent/Error/Completed/Failed/DuplicateRejected）+ ProviderStatus；**每事件带 monotonic seq**；客户端发现跳号 → `GET /tasks/:id` 拉快照补齐；状态快照推送 1s 节流；队列 256，满丢最旧非关键事件
- **凭证（D24）**：配置文件明文 + 权限位；keyring v2

---

## 13. RemoteProvider

```rust
pub struct ProviderRuntime { enabled, authenticated, quota_remaining, concurrency_limit, busy,
                             backoff_until, last_error }
pub trait RemoteProvider: Send + Sync {
    fn name(&self) -> &str;  fn capabilities(&self) -> Vec<Capability>;
    fn runtime(&self) -> ProviderRuntime;
    async fn refresh_auth(&self) -> Result<()>;
    async fn submit(&self, source: &DownloadSource) -> Result<ProviderTaskId>;
    async fn status(&self, id) -> Result<ProviderStatus>;
    async fn resolve(&self, id) -> Result<Vec<ResolvedRemoteFile>>;
    async fn remove(&self, id) -> Result<()>;
}
pub struct ResolvedRemoteFile { rel_path, url, size, etag, expires_at }
```

- 并发 ≤2；选择 = enabled ∧ authenticated ∧ quota>0 ∧ !backoff → debrid/115（官方 API）
- 直链过期：update_sources(≤3) → resubmit(≤2)
- 实现：mock / 115 / debrid；~~迅雷云盘~~ 不做

---

## 14. HttpEngine（v0.6：明确 reqwest 边界，D29）

**分工（D29）**：
- **传输 = reqwest**（HTTP/1.1+H2、重定向、Cookie/头、Basic/Bearer、代理、TLS、连接池——全交给 reqwest）
- **自研 = 调度层**：静态分块规划、多连接领取未分配段、.part 管理、ETag/Last-Modified 策略、重试退避、镜像轮换、`update_sources` 换源、ContentIdentity 校验、限速
- 自研不碰协议细节（不做 QUIC/H3 精细控制——v1 非目标）

**静态分块（D11/D25）**：`N = clamp(file_size/64MB, 2, 8)`；段不相交 → 无文件锁；不支持 Range → 单连接流式；运行中只给未分配段加连接（v2）。

**ETag 策略（M2）**：一致→续传；弱 ETag 归一化后先试 Range（206→继续）；Ignore Range(200)/416/Length 变化→重下；无 ETag→Last-Modified+Length 双校验。

**校验（Q-B5）**：ETag+Content-Length 为准；sha256 仅用户/源提供时启用；失败重下 1 次后降级接受（告警）。

**默认值表（D25）**：分块 64MB；连接 2–8（初始 4）；重定向 ≤10；重试 429/5xx 退避 1/2/4/8s×4、403→查认证、404→文件级失败；认证 Basic/Bearer（Digest v2）；代理/TLS 可配、TLS 系统根证书+可选自定义 CA/insecure。

---

## 15. FTP 引擎（并入 httpdl，feature-gated）

被动模式（PASV/EPSV）、REST 续传（.part）、421 退避；不支持 SFTP/FTPS 隐式/目录递归/FXP。

---

## 16. 多来源协同（v2 + 可行矩阵）

前提：逐字节一致；身份用 ContentIdentity（v2 加 PieceHashed）。

| 组合 | 可行性 | 实现 |
| :--- | :--- | :--- |
| BT + Web Seed | ✅ v1 | libtorrent 内建 |
| BT + 外部 HTTP 镜像（byte-identical） | ✅ v2 | PieceCoordinator（read_piece 校验；先单文件种子） |
| BT + 云 | ❌ | 仅整文件兜底 |
| HTTP 多镜像 | ✅ v1 | mirror |
| FTP 与 BT 协同 | ⚠️ | 无哈希校验，v2 后置 |

---

## 17. 里程碑（执行详见 TDD 计划）

M0 工具链+通道（含 cxx spike）→ M1 FFI 全量（alert ≤12）+btcore → M2 核心模型+调度 → M3 会话/输出 → M4a/b HttpEngine + M4c FTP → M5 Provider 链路 → M6 健康+事件+CLI/WS → 收尾（含最小 CI）。

---

## 18. 参考依据（keenable 已验证）

libtorrent 官方（Torrent_Status/Session/Resume_Data/Torrent_Handle/Alerts）；cbindgen（F0 反面）；**rqbit LICENSE = Apache-2.0（2026-08-16 raw 核查）**；cxx（https://cxx.rs/）；alist/xunlei-lixian/PeerBanHelper/qBittorrent-EE/DebridDownloader/rqbit/librqbit/FileCentipede/Mydm；用户调研 `docs/research/2026-08-16-thunder-offline-research.md`。

---

## 19. 风险与待验证

1. Windows libtorrent 构建 + 绑定工具选型（M0：cxx spike + 构建）
2. FFI 内存模型与 alert 生命周期（M1 ASAN；alerts_dropped 补快照）
3. resume 异步流（M3 断电-恢复）
4. 自研调度层 vs aria2（M4 test server 验证；不达标换 aria2 adapter）
5. 云直链过期（update_sources 链路）
6. 三阶段评估弱 metadata（60s 超时→手动兜底）
7. Windows 共享冲突（D19 重试+提示）
8. WS 背压（256 队列 + seq/快照重同步）
9. 最小 CI 与本地 BT 集成测试的平衡（CI 只跑纯 Rust crates）

## 20. 决策清单回填

**A(4)**：O1 自研✅（D29 澄清）／O2 CLI+WS✅／O3 本机✅／O6 0.5+禁双份✅
**B(13)**：Q-B1 完成即停+状态分离✅／B4 并发+队列✅／B2·B3 末段+大小对齐✅／B5 ETag+Length、重下1次降级✅／B6 v1 全选✅／B7 resume 4 时机✅／B8 Failed 保留✅／B9 手动兜底✅／B10 预检（分段公式 D36）✅／B11 跨盘✅／B12 凭证明文+权限✅／B13 WS 9 事件+seq✅／B14 单实例✅／B15 网络参数✅
**C(10)**：分块64MB／连接2–8／重试1·2·4·8×4／观察窗15s／stall 30s·1MB／update≤3／resubmit≤2／健康30s／背压256／日志10MB×5／限速不限✅
**D(4)**：TOML 冷加载✅／CLI 命令集✅／最小 CI（D35）✅／本地 BT seed✅
**v0.6 新增(9)**：D28 cxx spike／D29 reqwest 边界／D30 许可修正／D31 alert≤12／D32 PausingAwait 内部化／D33 Identity 两态／D34 token 黑名单／D35 最小 CI／D36 细节包✅
