# 多引擎智能调度下载器 — M0–M7 TDD 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. 每个里程碑内严格遵守"先写失败测试 → 运行确认失败 → 最小实现 → 运行确认通过 → commit"。

**Goal:** 按已冻结的设计（`2026-08-16-smart-downloader-design.md` v0.6，23 项决策 + 独立评审 12 条已处置）实现 v1：Rust 调度层 + libtorrent 薄内核 + HttpEngine（reqwest 传输 + 自研调度层）+ Provider 云兜底（默认关）+ CLI/WebSocket。

**Architecture:** 单 workspace 五 crate（core/btcore/httpdl/provider/daemon）+ 手写 C ABI `ffi/lt.h`（bindgen 生成 Rust 侧；绑定工具由 M0 spike 决出，D28）。BT 引擎=libtorrent FFI；HTTP/FTP=reqwest 传输 + 自研调度层；云=RemoteProvider→直链→HttpEngine。所有权边界 §9、状态机 §10、身份模型 §7、FFI 契约 §8（设计文档 v0.6）为唯一事实源。

**Tech Stack:** Rust 2021（tokio/serde/thiserror/reqwest/async-trait/bindgen）+ C++/libtorrent 2.x（CMake/vcpkg）。测试：`cargo test` + `cargo llvm-cov`（覆盖率）。本地测试基建：HTTP test server（axum）、最小 FTP server、真实 BT seeder（rqbit 或 libtorrent 自 seed）。

---

## 前置：评审上下文（给新模型）

- 项目：个人自用、代码可分享的智能下载器。统一接入 magnet/.torrent/http/ftp/thunder://，按能力路由，BT 热门走 libtorrent、冷门走云兜底（默认关）、HTTP/FTP 走自研引擎。
- 关键事实：libtorrent=BSD-3、有 peer 封禁/Web Seed/piece 级；rqbit=**Apache-2.0**（2026-08-16 官方 LICENSE 核查），弃选理由=BEP-19 支持有限、无逐 peer 封禁、较年轻（**非许可证**）；cbindgen 是 Rust→C 工具（本项目不用，`lt.h` 手写）；绑定工具（手写 C ABI vs cxx）由 M0 spike 决出（D28）；迅雷云盘/迅雷本地 BT 均不集成。
- 已完成：全部决策收口（设计文档 §1/§20）+ v0.6 评审处置 12 条（§1.1）；本文件是执行层，实现细节与设计文档冲突时以设计文档为准，先改设计文档再改本计划。

## 全局工程约定

- Workspace：`smart-downloader/Cargo.toml`（members: crates/core, crates/btcore, crates/httpdl, crates/provider, crates/daemon）
- 测试工具：`cargo install cargo-llvm-cov`（一次）；覆盖率命令 `cargo llvm-cov --workspace --html`（Windows 需 llvm-tools-preview：`rustup component add llvm-tools-preview`）
- 测试基建目录：`tests/integration/`（BT seeder、HTTP server、FTP server 共用）
- Commit 约定：每完成一个里程碑内一个任务即 commit，message 形如 `feat(m2): router matrix + tests`

## 里程碑接口契约（前一个的输出 = 后一个的输入）

| 里程碑 | 输出契约（crate::API） | 消费方 |
| :--- | :--- | :--- |
| M0 | `ffi/lt.h`（子集）+ `btcore::Bare{new,add_magnet,status}` | M1 |
| M1 | `btcore::{BtCore, TorrentStatus, PeerInfo, Alert, ResumeBytes}` + 全量 `lt.h` | M2（mock 参照）/ M3（resume）/ M6（health） |
| M2 | `core::{DownloadSource, Capability, DownloadEngine, EngineRegistry, DownloadTask, CanonicalId, ContentIdentity, TaskState, FallbackPolicy, Router, HeatEvaluator, StateMachine}` | M3 / M5 / M6 |
| M3 | `core::session::{SessionManager, OutputManager}`（消费 M1 resume + M2 types） | M6 |
| M4 | `httpdl::HttpEngine`（impl `DownloadEngine`） | M5 / M6 |
| M4c | `httpdl::FtpEngine`（feature=`ftp`） | M6 |
| M5 | `provider::{RemoteProvider, ProviderRuntime, MockProvider, FallbackCoordinator}`（消费 M2 策略 + M4 传输） | M6 |
| M6 | `daemon::{Cli, WsHub, SchedulerEvent}`（消费全部） | 用户 |

依赖图：`M0 → M1 → (M2 ∥ M3 ∥ M4) → M5 → M6`；M2/M3/M4 契约已定，可并行。

---

## M0 — 工具链与 BT 通道（2–4 天）

**目标**：Windows 上打通 `lt.h(手写) → lt_kernel.cpp → bindgen → Rust FFI` 全链路，真实磁力 progress>0。验证 F0 工具链方向与 libtorrent 构建可行性。

**接口契约（M0 输出，6 函数）**：
```c
/* ffi/lt.h 子集（M0：session + magnet + status + peer 注入） */
typedef struct lt_session lt_session;
typedef enum { LT_OK=0, LT_ERR_ARG, LT_ERR_ENGINE, LT_ERR_IO, LT_ERR_NOT_FOUND, LT_ERR_BUFFER_TOO_SMALL } lt_err;
lt_err lt_session_new(const char* save_path, const char* session_id, lt_session** out);
void   lt_session_free(lt_session* s);
lt_err lt_add_magnet(lt_session* s, const char* magnet, const char** web_seeds, char* ih_out /*41*/);
lt_err lt_status(lt_session* s, const char* ih, float* progress_out, int* state_out);
lt_err lt_add_peer(lt_session* s, const char* ih, const char* ip, uint16_t port); /* 本地 seeder 直连 */
```
- [ ] **Step 0: 绑定工具 spike（D28）**：手写 C ABI 与 cxx 各实现最小内核（`lt_session_create`/`lt_pop_alerts`/`lt_status`，各 ~200 行）→ 产出 `docs/superpowers/plans/2026-08-16-ffi-spike.md`（对比：构建复杂度 / 内存契约维护 / 异常与回调处理 / alert 扁平化工作量）→ **冻结 D14**（默认手写 C ABI + bindgen；cxx 若显著降低 lt_kernel.cpp 复杂度则改选）
- [ ] **Step 1: 写 M0 验收测试（先于任何实现）**
  - `scripts/m0/01_vcpkg.ps1`：`vcpkg install libtorrent:x64-windows`（含 boost/openssl 依赖），退出码 0 才算过。
  - `scripts/m0/02_build.ps1`：cmake 构建 `ffi/` + `cargo build -p btcore`，退出码 0。
  - `crates/btcore/tests/m0_magnet_e2e.rs`（真实磁力 E2E）：
    ```rust
    // 依赖 tests/integration/seed/ 起的本地 seeder（rqbit 做种 2MB 测试文件，含 DHT/tracker 本地地址）
    #[test]
    fn real_magnet_makes_progress_within_60s() {
        let seeder = TestSeeder::start();            // 返回本地 tracker 地址 + magnet
        let save = tempdir();
        let session = Bare::new(save.path().to_str().unwrap(), "m0").unwrap();
        let ih = session.add_magnet(&seeder.magnet(), &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut progress = 0.0;
        loop {
            let (p, _state) = session.status(&ih).unwrap();
            progress = p;
            if p > 0.0 { break; }
            assert!(Instant::now() < deadline, "progress stayed 0 for 60s");
            std::thread::sleep(Duration::from_millis(500));
        }
        assert!(progress > 0.0);
    }
    ```
- [ ] **Step 2: 运行确认失败**：`scripts/m0/01_vcpkg.ps1 && scripts/m0/02_build.ps1 && cargo test -p btcore --test m0_magnet_e2e` → 编译失败（无 lt.h/无 crate）即为预期失败。
- [ ] **Step 3: 最小实现**
  - `ffi/lt.h`（上面 5 个函数）；`ffi/src/lt_kernel.cpp`（session 包装 + add_magnet + status 轮询，内部用 libtorrent 2.x API）；`ffi/CMakeLists.txt`（输出静态库 `lt_kernel.lib`）。
  - `crates/btcore/build.rs`（bindgen 生成 `bindings.rs`，链接 lt_kernel）；`crates/btcore/src/ffi.rs`（extern 声明）；`crates/btcore/src/bare.rs`（`Bare` safe 包装，unsafe 只在这层）；`tests/integration/seed/mod.rs`（TestSeeder：rqbit 进程做种）。
- [ ] **Step 4: 运行确认通过**：同上命令 → 全部退出码 0，E2E 测试 PASS。
- [ ] **Step 5: Commit**：`feat(m0): libtorrent toolchain + bare ffi + magnet e2e`

**验收**：spike 对比文档产出并冻结 D14；3 个脚本/测试全过（构建 ×2 + E2E ×1）。覆盖率不适用（基建层）。**超 2 天未通过 → 记录风险，回退方案：qBittorrent 进程底座（设计文档 D2 复核）**。

**M0 出口自检清单**：
- [ ] `2026-08-16-ffi-spike.md` 对比报告已产出，D14 已冻结（☐ 手写 C ABI / ☐ cxx）
- [ ] 工具链脚本 `01_vcpkg.ps1`、`02_build.ps1` 退出码 0
- [ ] `m0_magnet_e2e` PASS（本地 seeder，60s 内 progress>0）
- [ ] `torrent_finished → lt_pause` 验证结论写回设计文档 §10.1（2.0.x 可靠性）
- [ ] alert ≤12 种字段级 schema 终稿（设计文档 §8.5）
- [ ] M1 可开工检查表：lt.h 全量 + btcore 骨架可编译、ASAN 配置就绪

---

## M1 — FFI 全量 + btcore（3–5 天）

**目标**：§8（设计文档 v0.6）全 ~28 函数落地；富 peer；**alert 扁平化 v1 预算 ≤12 种（§8.4 清单，其余 C++ 侧丢弃并计数）**；resume 异步流；内存模型测试 + ASAN。

**接口契约（M1 输出）**：`btcore::{BtCore, TorrentStatus, PeerInfo, Alert, ResumeBytes}`（safe API，unsafe 全在 `ffi.rs`）。

- [ ] **Step 1: 写测试**（`crates/btcore/tests/` + `crates/btcore/src/*_tests.rs`）
  - `ffi_memory_model.rs`：缓冲过小 → `LT_ERR_BUFFER_TOO_SMALL`；扩容重试成功；字符串立即拷贝后 C++ 侧修改不影响 Rust 值。
  - `alerts.rs`：alert 扁平化 → Rust 结构体（生命周期：pop 后旧缓冲不可用，Rust 持有拷贝）；`alerts_dropped>0` → 触发快照补拉路径。
  - `resume.rs`：`request_save_resume → (mock alert) → take_resume_data → 落盘 → add_torrent_resume` 往返一致。
  - `peers.rs`：`lt_peers` 富字段解析（client/peer_id/progress_ppm/flags 位）。
- [ ] **Step 2: 运行确认失败**（API 不存在，编译失败为预期）。
- [ ] **Step 3: 实现**：`ffi/lt.h` 全量 + `lt_kernel.cpp`（peer 枚举、alert ring buffer 1024、resume map）；`btcore/src/{ffi,alerts,resume,engine}.rs`。
- [ ] **Step 4: 通过**：`cargo test -p btcore` 全绿；`cargo llvm-cov -p btcore` ≥80%（Rust 侧）；另跑一轮 ASAN 构建集成测试。
- [ ] **Step 5: Commit**：`feat(m1): full ffi contract + alerts + resume flow`

**验收**：≥15 测试；btcore Rust 侧行覆盖 ≥80%；ASAN 一轮无报告。

---

## M2 — 核心模型 + 调度（3–4 天，纯 Rust，mock 引擎）

**目标**：能力模型、任务/文件模型、身份、所有权、三阶段评估、状态机（含 Queued 队列、Completed/Stopped 分离、手动兜底入口）、v1 启发式路由、去重。全部用 mock 引擎单测，不依赖 FFI。

**接口契约（M2 输出）**：`core` 上述类型 + `Router` + `StateMachine`。

- [ ] **Step 1: 写测试**（`crates/core/tests/`；mock 引擎 `tests/mocks/mock_engine.rs` 实现 `DownloadEngine`，可注入 peers/seeds/progress/error）
  - `router_matrix.rs`（用户指定 4 例 + 边界）：
    ```rust
    #[test] fn magnet_routes_to_bt() {
        let reg = registry_with(mock_bt(), mock_http());
        assert_eq!(reg.select(&DownloadSource::Magnet("m:".into())).unwrap(), "bt");
    }
    #[test] fn http_routes_to_http() { /* ... "http" */ }
    #[test] fn ftp_routes_to_ftp()  { /* "ftp"（feature 关闭时 → None） */ }
    #[test] fn ed2k_routes_to_failed() { /* None + Failed 事件 */ }
    ```
  - `state_machine.rs`：完整流转矩阵——`Queued→Evaluating→(MetadataPending→PeerDiscovery→HeatEvaluating)→Downloading→Completed→Stopped`；`Downloading→Stalled→PausingAwait→FallbackProvider→Transferring→Completed`；`*→Failed`；非法转换拒绝（`Queued→Completed` 等）；**Completed 不直接进 Seeding（默认）**。
  - `heat.rs`（用户指定公式边界）：
    ```rust
    #[test] fn zero_peers_zero_seeds_is_cold()  { assert!(score(0,0) < 0.3); }
    #[test] fn fifty_peers_ten_seeds_is_hot()   { assert!(score(50,10) >= 0.7); }
    #[test] fn middle_is_middle()               { let s = score(15,2); assert!((0.3..0.7).contains(&s)); }
    ```
  - `fallback_policy.rs`：BT 49% <0.5 → 允许兜底；51% → 拒绝自动兜底（仅手动）；`allow_parallel_disk=false` → 先 pause 再 Provider；**metadata 60s 超时 → 不触发 Provider，置 `FallbackAvailable` 标志**（Q-B9 写死）。
  - `dedup.rs`：同一 btih 重复 → `DuplicateRejected`；带 token URL 无 validator 不认重；有 size validator 一致才认重。
  - `queue.rs`：BT 并发配额 3、FIFO 顺序、HTTP 配额 8。
  - `identity.rs`：CanonicalId/ContentIdentity serde 往返；**ContentIdentity v1 仅 InfoHash/SingleFile 两态（D33）**——PieceHashed 在 v1 编译期不可用，v2 走 schema version 升级
  - `thunder.rs`（§7.1，D36）：`thunder://` 解码 = 去前缀 → base64 decode → 剥 `AA`/`ZZ` 壳 → 还原真实 URL；畸形输入 → 解析错误
  - `canonical_token.rs`（D34）：`?token=`/`?X-Amz-Signature=`/`?expires=`/`?auth=` 从 identity 剔除；`?v=1` 未命中黑名单 → 参与 identity；带 token 无 validator 不认重，有 size validator 一致才认重
- [ ] **Step 2: 运行确认失败**。
- [ ] **Step 3: 实现**：`core/src/{types,source_parse,task,ownership,registry,state_machine,heat,router,dedup}.rs`（纯逻辑，无 IO）。
- [ ] **Step 4: 通过**：`cargo test -p core`；`cargo llvm-cov -p core` ≥85%。
- [ ] **Step 5: Commit**：`feat(m2): core model + scheduler + routing`

**验收**：≥35 测试；行覆盖 ≥85%。本里程碑输出是 M3/M5/M6 的类型基础，**任何后续里程碑不得改 core 公开签名**（改需先改设计文档）。

---

## M3 — 会话/输出（2–3 天）

**目标**：state.json 持久化/恢复、resume 全流程（M1 能力）、.part 管理、Windows rename→copy fallback、磁盘预检、单实例锁。

**接口契约（M3 输出）**：`core::session::{SessionManager, OutputManager}`。

- [ ] **Step 1: 写测试**
  - `session_roundtrip.rs`：DownloadTask（含 files/acquisitions）save→load 字段一致；崩溃恢复场景（写一半的 state.json → 忽略并重建）。
  - `resume_flow.rs`：暂停/完成/退出 + 10min 定时触发保存（用 mock alert 源）；resume 文件损坏 → 报错不崩溃，重建任务。
  - `part_mgmt.rs`：.part 命名/长度校验；完成 rename 进下载目录；跨盘（temp 两个不同盘）rename 失败 → copy fallback + 删源。
  - `disk_precheck.rs`（D36 分段公式）：`required = max(total×1.1, total + min(500MB, total))`；10MB 文件 → 20MB；1GB 文件 → 1.1GB；剩余 < required → 拒绝入队
  - `single_instance.rs`：lock 文件已存在 → 新进程转发任务后退出。
- [ ] **Step 2/3/4/5**：先失败 → 实现 `core/src/session/{manager,output}.rs` → `cargo test -p core` + 覆盖率 ≥80% → commit `feat(m3): session & output`.

**验收**：≥20 测试；行覆盖 ≥80%。

---

## M4 — HttpEngine（M4a 骨架 3–4 天 + M4b 增强 3–4 天）

**目标（M4a）**：Range 探测、静态分块（单连接）、.part、ETag 优先续传、重试退避。**分工（D29）：传输用 reqwest（协议/重定向/头/认证/代理/TLS 全交给 reqwest），自研只做调度层（分块/续传/重试/换源/镜像/校验），不重写 HTTP 协议细节。**
**目标（M4b）**：多连接并行、镜像、`update_sources` 换源、ContentIdentity 校验、限速。

**接口契约（M4 输出）**：`httpdl::HttpEngine`（impl `DownloadEngine`；`update_sources` 实现在 M4b）。

**测试基建**：`tests/integration/http_server.rs`（axum 起可配置行为：206/200/416/429/中途 404/慢速段/ETag 变化）。

- [ ] **Step 1: 写测试**（`crates/httpdl/tests/`）
  - `range_probe.rs`：服务器支持 Range（206）→ 多连接；不支持（200）→ 单连接流式。
  - `split_plan.rs`：`plan(100MB)=2, plan(1GB)=4, plan(10GB)=8`；段互不相交且覆盖全文件。
  - `multi_conn_integrity.rs`：**4 段并行下载 64MB → 文件 SHA256 与源一致**（用户指定用例）。
  - `resume_etag.rs`：.part 存在 + ETag 一致 → 从偏移续传（服务器记录 Range 起点）；ETag 不一致 → 先试 Range，服务器 206 → 继续；服务器 Ignore Range(200)/416/Length 变化 → 重下。
  - `backoff_429.rs`：服务器先回 429×2 再 200 → 断言退避延迟序列 1s/2s 后成功（**不真等 3s，用可注入时钟**）。
  - `mirror_failover.rs`：mirror1 中途 404 → mirror2 接管，文件完整。
  - `update_sources.rs`（M4b）：直链过期（expires_at 到/404）→ `update_sources` 新 URL 继续，未完成段续传；新 ETag 不一致 → 单文件 .part 作废重下，其他文件不受影响。
  - `verify.rs`（M4b）：提供 sha256 → 校验失败重下 1 次 → 仍失败 → **降级接受 + 告警**（Q-B5）。
- [ ] **Step 2: 运行确认失败**。
- [ ] **Step 3: 实现**：M4a `httpdl/src/{range,static_split,resume,retry,engine}.rs`；M4b 加 `{multi_conn,mirror,sources,verify}.rs`。
- [ ] **Step 4: 通过**：`cargo test -p httpdl`；覆盖率 ≥75%；**若不达标 → 记录，按设计文档 D11 换 aria2 adapter 预案**。
- [ ] **Step 5: Commit**：`feat(m4a): http skeleton` / `feat(m4b): http multi-conn + mirrors + sources`.

**验收**：M4a ≥12 测试；M4b ≥13 测试（合计 ≥25）；行覆盖 ≥75%。

---

## M4c — FTP 模块（2 天，feature=`ftp`）

- [ ] **Step 1: 写测试**（`crates/httpdl/tests/ftp_*`；本地最小 FTP server 实现 PASV/REST）：连接+被动模式下载小文件；REST 续传（.part 存在 → 从偏移续）；421 退避重试；目录 URL → Failed（v1 不支持）。
- [ ] **Step 2/3/4/5**：失败 → 实现 `httpdl/src/protocol/ftp.rs` → `cargo test -p httpdl --features ftp` 全绿 → commit `feat(m4c): ftp engine`.

**验收**：≥8 测试；行覆盖 ≥70%。

---

## M5 — Provider 链路 + FallbackPolicy 集成（3–4 天）

**目标**：mock Provider 全生命周期；配额/backoff/直链过期注入；与 M2 策略、M4 传输的集成（BT 半成品保留、禁双份占盘）。

**接口契约（M5 输出）**：`provider::{RemoteProvider, ProviderRuntime, MockProvider, FallbackCoordinator}`。

- [ ] **Step 1: 写测试**（`crates/provider/tests/`）
  - `mock_lifecycle.rs`：submit→Queued→Downloading→Ready→resolve（返回 2 文件）→ HttpEngine 传输 → Completed。
  - `quota_backoff.rs`：quota=0 或 backoff 中 → Router 不选该 Provider；注入"quota 耗尽"场景。
  - `link_expiry.rs`：Transferring 中直链过期 → update_sources(≤3) → resubmit(≤2) → 超限 Failed。
  - `fallback_integration.rs`（关键）：BT stall 且 <50% → `lt_pause` 后启动 Provider → 完成 → **断言 BT 半成品文件仍在（未被删）**；`allow_parallel_disk=false` → Provider 在 pause alert 之后才启动（无双份）；BT ≥50% → 拒绝自动兜底。
- [ ] **Step 2/3/4/5**：失败 → 实现 `provider/src/{runtime,mock,coordinator}.rs` → `cargo test -p provider` 覆盖率 ≥75% → commit `feat(m5): provider chain + fallback policy`.

**验收**：≥15 测试；行覆盖 ≥75%。**BT 半成品保留断言为硬性验收**（对应 F3 修复，回归测试常驻）。

---

## M6 — 健康 + 事件 + CLI/WS（2–3 天）

**目标**：富 peer 反吸血记录、RatioLow、9 类事件、WS 背压、CLI 命令集、monitor_loop 组装。

**接口契约（M6 输出）**：`daemon::{Cli, WsHub, SchedulerEvent}`。

- [ ] **Step 1: 写测试**
  - `events.rs`：9 类事件（TaskCreated/StateChanged/Progress/Speed/HealthEvent/Error/Completed/Failed/DuplicateRejected）serde 往返 + 字段对齐。
  - `ws_backpressure.rs`：消费者慢 → 队列 256 上限，丢最旧非关键事件；**每事件带 monotonic seq**；客户端发现跳号 → `GET /tasks/:id` 拉快照补齐（D36）；掉队客户端重连 → 快照重同步。
  - `cli.rs`：8 命令解析（add/pause/resume/remove/list/status/logs/config）+ `--json` 输出；`fallback <task_id>` 手动兜底命令（Q-B9 入口）。
  - `health_leech.rs`：注入 `-XL0012-…` / client "Xunlei 0.1.0" 的 PeerInfo → `LeechDetected`；正常 `-qB…` 不触发。
  - `ratio_low.rs`：累计 `sum(total_upload)/sum(total_download)` < 0.5 → `RatioLow`；≥0.5 不触发。
- [ ] **Step 2/3/4/5**：失败 → 实现 `daemon/src/{cli,ws,monitor}.rs` → `cargo test -p daemon` 覆盖率 ≥75% → commit `feat(m6): cli + ws + health`.

**验收**：≥12 测试；行覆盖 ≥75%。`cargo test --workspace` 全绿（合计 ≥130 测试）。

---

## 收尾（M6 后 1 天）

- [ ] `cargo llvm-cov --workspace --html` 出报告，对照各里程碑阈值核查
- [ ] 全 workspace `cargo build --release` 无警告（deny warnings 可选）
- [ ] 最小 CI（D35）：`.github/workflows/ci.yml` = `fmt --check` + `clippy -D warnings` + `cargo test -p core -p httpdl -p provider -p daemon`（纯 Rust crates，无 libtorrent）；BT 集成测试（需 libtorrent 构建）留本地/后置独立 job
- [ ] 设计文档 v0.5 → v1.0（实现阶段实际偏差回填）
- [ ] 决策清单 §18 全部 ✅ 状态核对

## 风险对照（实现期重点关注）

| 风险 | 里程碑 | 预案 |
| :--- | :--- | :--- |
| Windows libtorrent 构建/bindgen | M0 | 2 天超时 → qBittorrent 进程底座 |
| FFI 内存模型/alert 生命周期 | M1 | ASAN + 快照补拉路径 |
| resume 异步流 | M3 | 断电-恢复集成测试常驻 |
| 自研 HttpEngine 不达标 | M4 | 换 aria2 adapter（D11 预案） |
| 云直链过期 | M5 | update_sources/resubmit 链路测试 |
| WS 背压 | M6 | 256 队列 + 快照重同步 |
