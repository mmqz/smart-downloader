# .xlbt.cfg / .bt.xltd 格式规范 — 真实样本验证版 (A 级)

> **状态**: ✅ **已验证** — 基于真实样本 `C5AA149AE0776344A270EAFEE49FDADB43FF6097`
> (audio-books-cjk 任务, 下载至 ~83%, 2263 pieces @131072)
> **验证日期**: 2026-08-17
> **验证工具**: `tools/xunlei-migrate/validate_xunlei_sample.py` V1-V8 全绿
> **样本位置**: `tools/xunlei-migrate/samples/` (torrent + cfg + cover.jpg.bt.xltd; 296MB m4b.xltd 在原始目录)

---

## 证据等级定义

| 等级 | 含义 | 可否写入生产代码 |
|---|---|---|
| **A** | 真实样本逐字节验证 (hexdump + SHA1 交叉) | ✅ 可写, 保留版本号 |
| **B** | 多个独立证据一致支持 | ⚠ 可写, 带 fallback |
| **C** | 单一间接证据 / 推断 | ❌ 禁止写死 |
| **D** | 纯命名推测 | ❌ 禁止写死 |

---

## 1. .xlbt.cfg 文件头 (真实布局, A 级)

| 偏移 | 大小 | 字段 | 值 (样本) | 说明 |
|---|---|---|---|---|
| 0x00 | 8B | magic | `XDLCTX\x00\x00` | **注意: 非旧推测的 "XLBTCFG"** |
| 0x08 | 16B | 任务随机区 | 16 字节 opaque | 语义未知 (疑似 task uuid) |
| 0x18 | 4B | u32 | 30025 | 语义未知 (与 piece 数相关候选) |
| 0x1C | 4B | u32 | 7 | 语义未知 |
| 0x20 | 4B | u32 | 0 | 语义未知 |
| 0x24 | 4B | u32 | 4 | 语义未知 |
| 0x28 | 4B | u32 | 0 | 语义未知 |
| 0x2C | 4B | u32 | 28584 | 语义未知 |
| 0x30 | 4B | u32 | 4 | 语义未知 |
| 0x34 | 4B | u32 | 262145 | 语义未知 |
| 0x38 | 4B | u32 | 40 | infohash 字符串长度 |
| 0x3C | 40B | infohash | ASCII 大写 hex | = torrent v1 info_hash (V7 验证) |

**已推翻的旧假设** (反汇编推断 vs 真实样本):
- ~~magic = "XLBTCFG\x00"~~ → 真实 `XDLCTX\x00\x00`
- ~~block_count / block_size 头部字段~~ → 不存在; 4096 对齐只体现在 .bt.xltd 尺寸
- ~~section_count / section 数组 (20B/entry)~~ → 不存在; 真实结构为 tag-02/tag-04 TLV 记录

---

## 2. .xlbt.cfg 内容记录 (A 级: 已确认部分)

### tag-02 int 记录: `02 00 <key:le16> <val:le32>`, 8B/entry, 自 0x64 起

| key | 语义 | 样本值 | 验证方式 |
|---|---|---|---|
| 1 | **已下载 piece 数** | 1868 | 与 .bt.xltd SHA1 验算 (1866 完成 + 16 在途) 交叉吻合 (V2) |
| 2..2200 | 保留 | 0 | - |

### 内嵌 u64 文件大小 (A 级)

- 0x6F7C: 740,642 (= cover.jpg size)
- 0x499C: 295,849,204 (= .m4b size)

### peer 缓存 (A 级)

`bt://<ip>:<port>` 字符串记录 (样本 8+ 条), 迅雷 BT peer 缓存 (可作转换后补种的 peer 提示)。

### tag-04 blob 记录: `04 00 <len:le32> <data>`

- 231 个 20B blob, **无一匹配 torrent piece 哈希** → 非 piece 哈希表
- "Reserved" 8B 标签 ×4 (0x700C 起)
- 0x71BA 起 peer 表; 64KB 粒度块记录 (0x4968 起, `65536×n+2` 序列) — 语义未完全解码

### 关键否定结论 (A 级, V5/V6)

- cfg **无 bitfield** (完成位图) — 2263×20=45KB piece 哈希 + 283B 位图均不存在于 32KB cfg
- cfg **无 piece 哈希表** — 容量论证 + 231 blob 零匹配双重证明
- 下载状态表达在 **.bt.xltd 的零区** (见 §4)

---

## 3. .bt.xltd 文件结构 (A 级, 已验证)

### 核心模型: 文件的位置镜像

```
.bt.xltd 字节偏移 x  ≡ 目标文件字节偏移 x   (同一文件)
大小 = ceil(file_size / 4096) × 4096       (4096 对齐, 样本双文件精确命中)
无文件头 magic                              (首字节即 piece 数据)
整文件预分配, 未下载区域零填充                (非 NTFS sparse: fsutil queryAllocRanges 显示全量分配)
```

### piece 数据物理偏移公式 (V4, SHA1 验证 1866 命中 / 99.1%)

对 torrent 文件 f (起始字节偏移 `file_offset`, piece_length `plen`):
```
内部 piece p 的 xltd 偏移 = p × plen − file_offset        (p 完全落在 f 内)
边界 piece (跨多文件) 无法从单个 xltd 验证 — 设计内排除
```

验证方法: 从 xltd 读该窗口 → SHA1 → 与 torrent pieces_hash 比对 (命中率 ≥ 80% 且 ≥ 30 个)。

### 完成状态判定

- SHA1 一致 → piece 完成
- 窗口全零 → 未下载
- 部分非零但哈希不一致 → 在途 (下载中, 转换时视为未完成)

### 已推翻的旧假设

- ~~sparse hole 表达未下载~~ → 实际是全量分配 + 零填充
- ~~可能有文件头~~ → 无头, 尺寸公式铁证 (741,376 = ceil(740642/4096)×4096 等)

---

## 4. GCID/CID/BCID (沿用 A 级开源结论, 与本格式无耦合)

- GCID: xlgcid-python 算法 (piece_size 256KB 起动态翻倍, 二次 SHA1)
- BT 任务 CID = BTIH (binux 2012)
- BCID: 未公开; 对 BT 接续非必需 (piece SHA1 + bitfield 就够)

---

## 5. 验证状态总表

| # | 问题 | 原等级 | 验证后 | 证据 |
|---|---|---|---|---|
| V1 | cfg 结构 (magic + infohash 字段) | D | **A** | magic=XDLCTX\0\0, infohash@0x3c ASCII |
| V2 | key=1 int 字段语义 | C | **A** | = 已下载 piece 数, 与 SHA1 交叉 1868≈1882 |
| V3 | .bt.xltd 是否有头部 | B | **A** | 无头; 尺寸 = ceil(file/4096)*4096 (双文件) |
| V4 | piece 物理偏移公式 | C | **A** | p*plen−file_offset; SHA1 命中 1866/1882=99.1% |
| V5 | CXBitmap 字节序 | D | **A(否定)** | cfg 无 bitfield; 状态由 xltd 零区表达 |
| V6 | bitfield 每 piece 1bit | D | **A(否定)** | 不适用 (无 bitfield) |
| V7 | cfg info hash 校验 | C | **A** | 0x3c ASCII == torrent v1 infohash (不区分大小写) |
| V8 | block_count/block_size 语义 | C | **A(修正)** | 无该头部; 4096 仅对齐 xltd 尺寸 |

**遗留未解码 (B/C 级)**: 头部 0x08-0x3B 字段语义; tag-02 key 2..2200; 64KB 块记录语义;
20B blob (231 个) 内容; peer 记录内部字段。不影响 BT 接续转换。

---

## 6. 转换路径 (xunlei_to_libtorrent_converter.py)

1. 读 .torrent → piece_length / pieces_hash / info_hash / 文件偏移表
2. 读 .xlbt.cfg → magic + infohash 一致性校验 (任务归属)
3. 对每个 .bt.xltd (4096 对齐尺寸 ↔ torrent 文件) → 逐 piece SHA1 → 完成位图
4. 生成 libtorrent fastresume (v1): info-hash + pieces 位图 + file sizes
5. 数据文件缺失时从 xltd 物化 (去 4096 填充)
6. qBittorrent 侧: 添加 .torrent + 数据目录 → 自动 rehash → 补下缺失 piece

E2E: `e2e_test_converter.py` 合成真实格式样本 → 诊断 + 转换 + fastresume 回读全绿。

---

## 7. 变更日志

- 2026-08-16 初版: 反汇编推断冻结 (XLBTCFG / section 数组 / bitfield 假设)
- 2026-08-17 真实样本验证: 推翻 3 项核心假设, 确认真实格式 (XDLCTX / 位置镜像 / 零区状态), V1-V8 全绿
- 2026-08-17 工具重构: parse_xlbt_cfg / validate_xunlei_sample / xunlei_to_libtorrent_converter 全部真实格式化, e2e 通过

## 最后更新

2026-08-17 UTC+8
