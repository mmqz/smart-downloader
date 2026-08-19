# 已完成事项记录（IMPLEMENTED）

> 用途：**已落地功能**的集中档案（与 `BACKLOG.md` 的"未实现清单"互补）。
> 每条含 commit 与验证证据；"行为契约"为实操查阅要点（配置 / API / 语义）。
> 只记录事实与边界，不讨论设计。

## 通用能力（2026-08-19 批次）

### 1. 全局代理 + 下载/上传限速（`3fac8e3`，补充测试 `12b0101`）

**配置**（启动时生效，**不参与热重载**——避免重建连接）：

```toml
[download]
proxy = "http://host:port"        # 或 socks5:// / socks4://
max_download_kb_s = 2048          # 全局下载限速（HTTP + BT 共用）；0 = 不限
[bt]
max_upload_kb_s = 512             # BT 上传限速；0 = 不限
```

**行为契约**
- HTTP 引擎经 reqwest `Proxy::all` + 凭据（URL 内 `user:pass@` → basic_auth）；BT 引擎经内核 `lt_apply_network`（settings_pack，对齐 libtorrent 2.x：字段名 `proxy_hostname`）。
- 限速：跨段共享 `RateLimiter`（deadline 链）。**冷启动语义**：无积压时首个 chunk 立即放行（突发恢复不欠债），连续消费才节流。
- 敏感项：**proxy URL 不出现在 `/config` 快照**（仅暴露 `proxy_enabled`）。

**验证**：config 快照断言 5 用例、parse_proxy 5 用例、RateLimiter 时序 4 用例、引擎级限速集成 2 用例（限速生效对照 + 完整性 SHA256 一致）。

**已知边界**：代理实际转发需真实代理实连确认（本地仅验证解析/构建/启动路径）。

### 2. M6：云兜底调度接线（`c19313a`）

**API**：`POST /tasks/:id/fallback`（原 501 桩 → 真实执行）

**前置条件（FallbackPolicy 默认冻结）**
1. 任务必须是 **BT 任务**（HTTP/FTP 任务 → 409 "仅 BT 任务支持云兜底"）
2. 任务必须**已暂停**（串行策略：禁 BT/直链双份占盘 → 409 "需先暂停"）
3. BT 进度 **< 50%**（≥50% → 409 "仅进度 <50% 可兜底"）
4. 存在**可用 provider**（无 → 409 "无可用 provider"；不可用含未配置/未认证/配额耗尽/冷却/并发满）

**成功语义**：选 provider → submit → Ready → resolve 直链 → HttpEngine 传输（每个直链一个引擎任务，轮询终态，60s 超时）→ BT 引擎任务退役（keep data）→ 任务置 `Completed` + `StateChanged`/`Completed` 事件广播 + 落盘。响应 200：`{"status":"completed","provider","provider_task","transferred":[...]}`。
直链过期恢复：update_sources ≤3 → resubmit ≤2 → 超限 Failed（409）。

**配置**

```toml
[provider]
enabled = true   # 兜底总开关（默认 false：不自动烧配额）
mock = true      # 开发/演示 MockProvider（唯一现成实现；真实 provider 待迅雷线落地）
```

`GET /providers` 列出 provider 运行态（enabled/authenticated/quota/backoff/busy）。

**错误映射**：404（任务不存在）/ 409（上述语义错误）/ 500（其它引擎错误）。

**验证**：`fallback_api` e2e 2 用例（直链成功落盘 + SHA256 内容一致；未暂停 409；无 provider 409）、http_api 404/409 语义 2 用例、serve 冒烟（`[provider] mock=true` → `/config.provider_enabled=true` + `/providers` 列出 mock）。

**顺带修复（同 commit）**
- BT/magnet/.torrent 任务 dest 缺省值 = 配置 `dest_root`（原为 `.`，与 HTTP 不一致）
- pause/resume 后任务记录缓存同步（原 alert 流不迁移 pause，list/快照与 API 动作不一致）

**已知边界**：provider 目前只有 MockProvider（占位）；真实 provider（迅雷云盘等）属迅雷线，落地后按 `RemoteProvider` trait 注入即可，调度骨架已就绪。

### 3. http_api 轮询护栏修复（`051c8a4`）

**根因**：重负载窗口（连续重建 + 杀软扫描新编译 exe）的进程级停顿击穿轮询用例 10s 等待护栏 → 偶发单用例超时失败（曾观测 3 次，均"16 passed; 1 failed"；50+ 次运行 + 6 轮强制 rebuild 首跑全绿，与负载停顿吻合）。

**修复**：`crates/daemon/tests/http_api.rs` 三处等待护栏（快照 snapshot / list / 事件 event）**10s → 60s**。语义不变（最终一致性断言、100ms 轮询），对进程级停顿免疫。

**验证**：修复后 http_api 17/17、fmt 0、clippy 0。

## 主线历史（已完成，详见 git log）

- 迅雷/QQDL 链接容错解码（`a989bb8`）、HTTP 断点续传 `.part`+etag（`0252c4f`）、BT fastresume 显式保存（`4f33cd1`）、任务持久化+恢复（`3ce222a`）、TOML 热重载（`784a269`）、HTTP 任务状态推进轮询（`9dea1a0`）、CLI 执行层（`3dfb8dd`）、端点补齐（`df82dfb`）等。
- **迅雷云盘线（F2/F2.1/F3…）外包专用会话，状态见 `BACKLOG.md` A 段。**