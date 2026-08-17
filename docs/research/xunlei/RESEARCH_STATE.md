# Research State - 最终状态 (真实样本验证完成)

## 总研究目标

确认"完全不依赖迅雷黑盒 DLL"是否可行,同时彻底搞清:
1. ✅ 迅雷 BT 下载文件"不通用"的根本原因 (已查清)
2. ⚠ 是否能原生接入迅雷 P2P 网络 (已评估, 不推荐)
3. ✅ 推荐 libtorrent + 转换器组合方案 (已确立)
4. ✅ 是否能写"迅雷 → libtorrent 转换器" (真实样本验证完成)

## 当前阶段

**研究完成 + 真实样本验证完成** (2026-08-17)。

## 真实样本验证结果 (Round 5b, 2026-08-17)

用户提供真实任务样本 (audio-books-cjk, infohash C5AA149AE0776344A270EAFEE49FDADB43FF6097,
2263 pieces @131072, ~83%)。`validate_xunlei_sample.py` V1-V8 **全绿**, 其中 3 项
核心反汇编推断被**推翻并修正**:

| 项 | 旧推断 (反汇编) | 真实样本 (A 级) |
|---|---|---|
| cfg magic | "XLBTCFG\x00" | **"XDLCTX\x00\x00"** |
| cfg 结构 | 40B 头 + section 数组 (20B/entry) | **TLV 记录** (tag-02 int / tag-04 blob), infohash ASCII @0x3c |
| cfg 内容 | 含 piece 哈希表 + bitfield | **无** (32KB cfg 物理装不下 45KB 哈希; 231 个 20B blob 零匹配) |
| .bt.xltd | 纯 piece 数据 sparse file | **文件位置镜像**: 无头, 尺寸=ceil(file/4096)*4096, 全量分配+零填充 |
| piece 偏移 | idx×piece_length | **p×plen−file_offset** (内部 piece), SHA1 命中 1866/1882=99.1% |
| 完成状态 | cfg bitfield | **xltd 零区** + torrent 哈希 SHA1 推导 |

## 已完成目标 (Round 1-5)

### Phase 1: 基础逆向 ✅
- DLL 解包, 100 个 XL_* 导出, BT 协议类簇, ABI 推断

### Phase 2 Round 1-4: 哈希类/网络/格式反汇编 ✅
- F1-F30: m_piecesHash / GCID-CID 算法 / XLBTCFG 头部 / BT_PURE_DataBlockReader
  (细节见 spec_pending_validation.md; 部分头部推断已被真实样本修正)

### Phase 2 Round 5: CXBitmap 反汇编 + PoC ✅
- CXBitmap 内部是 std::string; PoC 解析器/探测器跑通 (合成文件)
- ⚠ 真实样本修正: cfg 中无 bitfield; CXBitmap 结论不适用于 cfg 持久化格式

### Round 5b: 真实样本验证 ✅ (2026-08-17)
- 样本落库: `tools/xunlei-migrate/samples/` (torrent + cfg + cover.jpg.bt.xltd)
- 工具重构为真实格式: parse_xlbt_cfg / validate_xunlei_sample / xunlei_to_libtorrent_converter
- e2e 合成真实格式样本全流程通过 (诊断 + 转换 + fastresume 回读)
- 遗留: cfg 头部 0x08-0x3B / key 2..2200 / 64KB 块记录 / 20B blob 语义 (B/C 级, 不影响转换)

## 结论

- 迅雷 → libtorrent 转换器**可行且已验证**: 数据在 .bt.xltd 位置镜像中,
  完成位图可 SHA1 推导, fastresume 可生成, qBittorrent rehash 补下缺失 piece
- 转换器需用户提供 .torrent (piece 哈希的唯一来源; 迅雷只存任务元数据不存哈希)
