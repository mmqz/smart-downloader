# Findings (Round 3 完成 + Round 4 新发现)

## Round 3 (已固化)
F13-F25 (见上文)

## Round 4 (新) - .bt.xltd 文件格式推断 ★★★

### F26 (B 级) - .bt.xltd 是纯 piece 数据文件

**证据**:
1. 字符串 `'BT_PURE_DataBlock_Reader'` 在 XLBTFileOutputDataSourceImpl::vtable[6] 中引用
2. 类名 `BTPureDataBlockReader` 暗示 "纯数据读取器"
3. DownloadSDK.dll 中除 XLBTCFG 外无其他 ASCII magic (movabs 加载的)
4. `GetBTTempDataFileSuffix` 是极简函数,只返回 ".bt.xltd" 字符串 (3 条指令)

**含义**: `.bt.xltd` 文件**没有 ASCII magic**,可能是:
- 选项 A: 直接是 piece 数据 sparse file,按 `piece_index * piece_length` 偏移存储
- 选项 B: 有简单二进制头 (例如 4 字节 version),无 ASCII magic
- 选项 C: 与 .xlbt.cfg 共享同一格式 (用 XLBTCFG magic 但内容不同)

**最可能**: 选项 A,因为 BTPureDataBlockReader 的命名暗示"纯数据"。

### F27 (A 级) - BTPureDataBlockReader vtable 结构
- vtable @ 0x1803b2520
- [0] = 0x1802cd790 (含 immediate 0x20=32, 0x140=320)
- [1] = [2] = 0x180005bd0 (空方法, ret 0)
- 总 size 推测 ~0x140 (320 字节)

### F28 (A 级) - BTDataBlockReader vtable (基类)
- vtable @ 0x1803b2320
- 多个方法 immediate 0x20 (32), 0x40 (64), 0x130 (304) 等
- 这些是 sub-object size 或 buffer size

### F29 (B 级) - XLBTFileOutputDataSourceImpl 是 .bt.xltd 写入器
- vtable @ 0x1803a79a0
- [6] 引用 'BT_PURE_DataBlock_Reader' 字符串
- 含义: 该类负责通过 BTPureDataBlockReader 写入 .bt.xltd

### F30 (C 级) - piece 数据偏移计算
类 `BTPureDataBlockReader::IntersectingPieceInfo::GetOffsetToData` 存在
- 含义: piece 数据通过偏移量定位 (而非查表)
- 推断: offset = piece_index × piece_length (标准 BT 规范)

## 修正后的路径 D 可行性评估

| 子任务 | 状态 | 证据等级 |
|---|---|---|
| GCID 算法 | ✅ 已公开 | A |
| CID 算法 | ✅ 已公开 | A |
| BT piece SHA1 算法 | ✅ 标准 | A |
| BT 任务 CID = BTIH | ✅ 已确认 | A |
| .xlbt.cfg magic | ✅ 已破解 (XLBTCFG) | A |
| .xlbt.cfg 头部结构 | ✅ 已破解 (40 字节 + sections) | A |
| section 内容布局 | ⚠ 待逆向 | C |
| .bt.xltd magic | ⚠ 无 ASCII magic,可能纯数据 | B |
| .bt.xltd 物理布局 | ⚠ 推断为按偏移存储 | C |
| CXBitmap 格式 | ❌ 未确认 | D |
| 写一个能用的转换器 | ⚠ 算法层可行,工程层基本可行 | B- |

## 关键洞察修正

### 修正前: ".xltd 含 BCID 哈希表,需要完全逆向才能读取"
### 修正后 (F26-F30):

1. **.bt.xltd 可能是纯 piece 数据 sparse file** (B 级)
2. **piece 数据按 `piece_index * piece_length` 偏移存储** (C 级推断)
3. **完成位图在 .xlbt.cfg 中** (推测,待 F14 section 内容确认)
4. **不需要逆向 BCID 即可读取 piece 数据**

### 重大含义

如果 .bt.xltd 是纯 piece 数据:
- 直接复制为 .part 文件
- 从 .xlbt.cfg 读 piece hash + piece length + 完成位图
- 用 piece hash 校验已下载 piece
- 生成 libtorrent fastresume

**这就是转换器的最小可行路径**!

## 路径 D 实际工作量重估

| 任务 | 工作量 | 风险 |
|---|---|---|
| 写 .xlbt.cfg 解析器 (magic + 头部) | 1 天 | 低 (已破解) |
| 写 .xlbt.cfg section 解析器 | 2-3 天 | 中 (待逆向) |
| 写 .bt.xltd → .part 转换 | 1 天 | 低 (如果是纯数据) |
| 写完成位图 → libtorrent fastresume | 1-2 天 | 中 (CXBitmap 格式) |
| 测试 + 调试 | 2-3 天 | 中 |
| **总计** | **7-10 天** | 中 |

## 仍未验证

1. **.bt.xltd 是否真的没有头部** (需要真实样本 hex 验证)
2. **每个 section_id 对应什么内容** (需要继续反汇编 LoadCfgData)
3. **CXBitmap 二进制格式** (位图存储)

## 用户决策点 (现在可以问了)

根据规则,这是"必须由用户提供文件"的情况:
- 反汇编已到极限
- 真实文件样本能立即验证所有推断
- 没有样本,继续反汇编 ROI 低

**建议让用户**: 跑一个迅雷 BT 任务,下载到 50% 时停止,把 `<filename>.bt.xltd` + `<filename>.xlbt.cfg` + 原始 .torrent 文件给我做 hex 分析。

## 最后更新时间

2026-08-16 14:50 UTC+8
