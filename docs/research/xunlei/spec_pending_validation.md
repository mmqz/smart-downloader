# .xlbt.cfg / .bt.xltd 格式规范 — 待验证版

> **状态**: ⚠ **未验证** — 所有字段均为反汇编推断,无真实样本验证
> **目的**: 锁定推断结果,禁止将未验证假设写入生产代码
> **依据**: 反汇编 DownloadSDK.dll v25.0.90.1592 + 开源参考 (xlgcid-python, xunlei-lixian)
> **创建**: 2026-08-16
> **验证状态**: 待用户提供真实 .xltd/.cfg/.torrent 样本

---

## 证据等级定义

| 等级 | 含义 | 可否写入生产代码 |
|---|---|---|
| **A** | 直接反汇编验证 + 开源交叉印证 | ✅ 可写,但需保留版本号 |
| **B** | 多个独立证据一致支持 (反汇编 + 字符串 + 类名) | ⚠ 仅可写,带 fallback |
| **C** | 单一间接证据 / 推断 | ❌ 禁止写死,必须放假设分支 |
| **D** | 纯命名推测 | ❌ 禁止写死,仅作探索 |

---

## 1. .xlbt.cfg 文件头 (40 字节)

### 字段布局

| 偏移 | 大小 | 字段名 | 类型 | 置信度 | 反汇编依据 |
|---|---|---|---|---|---|
| 0x00 | 8B | `magic` | byte[8] = `"XLBTCFG\x00"` | **A** | `movabs rax, 0x47464354424c58` @ 0x1802cbd74 (写) + `cmp rcx, rax` @ 0x18020c0fc (读校验) |
| 0x08 | 2B | `reserved1` | uint16 | **A** | `mov word ptr [rdi + 0x80], r15w` (r15=0, 写入路径) |
| 0x0A | 2B | `reserved2` | uint16 | **C** | 头部位置推断,无独立证据 |
| 0x0C | 4B | `reserved3` | uint32 | **C** | 同上 |
| 0x10 | 8B | `block_count` | uint64 LE | **A** | `mov qword ptr [rdi + 0x88], rcx` + 读路径 `cmp rax, [rdi+0x88]` |
| 0x18 | 8B | `block_size` | uint64 LE | **A** | `mov qword ptr [rdi + 0x90], r8` + 读路径 `test rcx, 0xfff` (4096 对齐检查) |
| 0x20 | 4B | `section_count` | uint32 LE | **A** | 循环上限 `cmp esi, [rdi+0x98]` @ 0x18020c1cc |
| 0x24 | 4B | `reserved4` | uint32 | **C** | 头部剩余字段,无独立证据 |

**约束**:
- `block_size` 必须是 4096 倍数 (反汇编 `test rcx, 0xfff; jne fail`)
- `magic` 严格 `"XLBTCFG\x00"`,任何不匹配直接拒绝加载

### 读写位置说明

写入对象内部偏移 `+0x78`(写时 base),但写入文件的物理偏移为 0x00。即:
```
struct XLBTCfgManager {  // C++ 对象, 偏移 = 文件偏移 - 0x78
    ... 其他字段 (0x00 ~ 0x77)
    uint8_t  magic[8]      @ +0x78  // = 文件 0x00
    uint16_t reserved1     @ +0x80  // = 文件 0x08
    ...
}
```

---

## 2. .xlbt.cfg Section 数组

### 字段布局 (每 entry 20 字节)

| 偏移(在 entry 内) | 大小 | 字段名 | 类型 | 置信度 | 反汇编依据 |
|---|---|---|---|---|---|
| 0x00 | 4B | `section_id` | uint32 LE | **A** | `mov eax, [rbx]` @ 0x18020c170 |
| 0x04 | 8B | `field2` (推测: size 或 offset) | uint64 LE | **C** | `mov rax, [rbx+4]`,语义未直接验证 |
| 0x0C | 8B | `field3` (推测: offset 或 reserved) | uint64 LE | **C** | `mov rax, [rbx+0xc]`,语义未直接验证 |
| (stride) | 0x14 | `next entry` | - | **A** | `add rbx, 0x14` @ 0x18020c188 |

### Section 数组存储位置

紧接 40B 头部之后,即文件偏移 `0x28` 起,共 `section_count × 20` 字节。

### 未验证点

- `field2` 与 `field3` 的具体语义未直接反汇编出来 (size? offset? 两者都有?)
- 当前 PoC 假设:`field2 = section body size`,`field3 = section body offset (绝对文件偏移)`,但**这是猜测**
- 真实样本验证时可一句话确认:看 `field2`/`field3` 数值是否在合理范围(size < 文件大小,offset < 文件大小)

---

## 3. ⚠ section_id → 内容映射 (重要未验证假设)

### 当前猜测

| section_id (猜测) | 推测内容 | 推测大小 | 置信度 | 推测依据 |
|---|---|---|---|---|
| 0x01 | `INFO_HASH` (BTIH, 20B) | 20 字节 | **D** | 纯命名推断,无反汇编证据 |
| 0x02 | `PIECES_HASH` (BT piece hash 列表) | num_pieces × 20 字节 | **D** | 字段名 `m_piecesHash` 存在,但 section_id 数值未确认 |
| 0x03 | `BITFIELD` (CXBitmap 完成位图) | ceil(num_pieces/8) 字节 | **D** | CXBitmap 类存在,但与 section_id 关联未确认 |
| 0x04 | `FILE_INFO` (文件名/大小) | 变长 | **D** | 纯命名推断 |
| 0x05 | `GCID` (20B) | 20 字节 | **D** | GCID 算法已公开,但 section_id 数值未确认 |

### 重要警告

**以上 section_id → 内容映射是纯猜测,随时可能全错。**

实际反汇编证据:
- `BTCfgManager::LoadCfgData` 反汇编看到 section 数组解析循环,但**没有 switch/case 按 section_id 派发**
- section 内容通过 `XPF_ParamStreamRead*` 系列函数读取
- 实际 section_id 数值需真实 .xlbt.cfg 样本 hex 验证

### 禁止事项

- ❌ **禁止**在任何生产代码里硬编码 `SECTION_ID_INFO_HASH = 0x01` 等常量
- ❌ **禁止**在转换器里假设 section 0 是 infohash
- ✅ **必须**在转换器里加 `validate_sample.py` 验证通过后才启用假设分支

---

## 4. .xlbt.cfg 后续校验:info hash

### 已知 (A 级)

- 字符串 `"cfg info hash not match!"` @ 反汇编 0x18020a051
- 错误码 `0x59da` (23002)
- `BTTask::GetInfoHash` 方法存在 (字符串证据)

### 未验证 (C/D 级)

- info hash 校验算法: 是 SHA1(bencoded info dict) 还是 SHA1(cfg 文件内容)?
- info hash 字段在哪个 section?
- 校验失败时是直接拒绝还是降级处理?

### 推测 (D 级)

info hash 校验可能是:
1. 从某个 section 读出 20 字节 infohash
2. 与 `BTTask::GetInfoHash()` 内存计算结果比对
3. 不一致则报 "cfg info hash not match!" 错误

**禁止**: 在没验证前,转换器**不能**依赖这个校验逻辑。

---

## 5. .bt.xltd 文件结构

### 已知 (B 级)

- 类 `BTPureDataBlockReader` 名字暗示"纯数据块读取器"
- 字符串 `'BT_PURE_DataBlock_Reader'` 在 `XLBTFileOutputDataSourceImpl::vtable[6]` 中引用
- `GetBTTempDataFileSuffix` 是极简函数 (3 条指令),只返回 `".bt.xltd"` 字符串
- DownloadSDK.dll 中除 `XLBTCFG` 外无其他 `movabs rax, IMM` 加载的 ASCII magic
- 知乎帖子 "迅雷 xltd 文件大小比占用空间大10倍" 印证 sparse file

### 未验证 (C/D 级)

- 是否真无文件头 magic (B 级推断"无",但未做真实文件 hex 验证)
- piece 数据物理偏移 = `piece_index × piece_length`? (C 级推断)
- 是否使用 NTFS sparse file 标志
- 是否有 per-piece 元数据 (如 BCID 哈希表内嵌)

### 推测布局

```
.bt.xltd 物理结构 (推测,未验证):

  如果是 sparse file + 标准偏移:
    offset 0: piece 0 数据 (piece_length 字节)
    offset piece_length: piece 1 数据
    offset 2*piece_length: piece 2 数据
    ...
    offset (num_pieces-1)*piece_length: 最后一个 piece
    未下载的 piece 区域为 sparse hole (0 字节实际占用)
  
  如果有头部:
    offset 0: 某种 magic + 元信息 (大小未知)
    offset ?: piece 0 数据
    ...
```

### 关键未验证问题

**Q11**: piece 数据物理偏移到底是 `piece_index × piece_length` 还是 `piece_index × block_size`(迅雷自有 block)?
- 如果是后者,且 block_size ≠ piece_length,则 libtorrent 无法直接接续
- 验证方法: 真实样本 + piece hash 比对

---

## 6. CXBitmap (完成位图) 内部结构

### 已知 (A 级, 反汇编 XLTaskUpgrade.dll)

```c
struct CXBitmap {  // sizeof = 0x18 (24 字节)
    void* vtable;            // +0x00
    void* internal_buffer;    // +0x08  (dtor 调 free 释放)
    std::string data;         // +0x10  (Windows STL, SSO 16 字节)
};
```

反汇编证据:
- vmethod0 (析构): `mov edx, 0x18; call free` (sizeof = 24)
- vmethod5: `add rcx, 0x10` + `cmp qword ptr [rdx+0x18], 0x10` (SSO 检查)
- vmethod11: 同样操作 `[rcx+0x10]` 处的 std::string

### 未验证 (C/D 级)

- `std::string` 内的字节序: big-endian (标准 BT) 还是 little-endian?
- 每 piece 1 bit (标准) 还是 1 byte (libtorrent 风格)?
- 是否有 `bitmap_count` / `Bitmap_len` 头部字段? (字符串证据 `"bitmap_count error"`, `"Bitmap_len error"`)

### 推测 (B 级)

CXBitmap 序列化结果**极可能是**标准 BT bitfield:
- 每 piece 1 bit
- big-endian 字节顺序
- 总大小 = `ceil(num_pieces / 8)`

**理由**: CXBitmap 内部是 std::string(可存储任意二进制),且迅雷 BT 协议层 90% 是标准的 (25 个 XBTPackage 类),bitfield 没理由不标准。

### 验证方法

真实 .xlbt.cfg 中找 BITFIELD section,看其大小:
- 若 = `ceil(num_pieces / 8)` → 每 piece 1 bit (标准)
- 若 = `num_pieces` → 每 piece 1 byte (libtorrent 风格)

---

## 7. GCID/CID/BCID 哈希算法

### GCID 算法 (A 级, 开源)

来源: https://github.com/Cologler/xlgcid-python

```python
def get_gcid_digest(fp, fp_size):
    h = hashlib.sha1()
    piece_size = 0x40000  # 256KB
    while fp_size / piece_size > 0x200 and piece_size < 0x200000:
        piece_size <<= 1
    # piece_size 动态: 256KB 起,文件 > 512 分片翻倍,上限 2MB
    
    while read := fp.readinto(buf):
        h.update(hashlib.sha1(buf[:read]).digest())  # 二次 SHA1
    return h.digest()  # 20 字节
```

### CID 算法 (A 级, 开源)

来源: https://github.com/iambus/xunlei-lixian (`lixian_hash.py::dcid_hash_file`)

```python
def dcid_hash_file(path):
    h = hashlib.sha1()
    size = os.path.getsize(path)
    with open(path, 'rb') as stream:
        if size < 0xF000:                # 文件 < 60KB: 全文件 SHA1
            h.update(stream.read())
        else:                            # 文件 >= 60KB: 头+中+尾各 0x5000
            h.update(stream.read(0x5000))             # 头部 0x5000 字节
            stream.seek(size/3)
            h.update(stream.read(0x5000))             # 中部 size/3 处 0x5000 字节
            stream.seek(size-0x5000)
            h.update(stream.read(0x5000))             # 尾部 0x5000 字节
    return h.hexdigest()
```

### BT 任务 CID = BTIH (A 级)

来源: binux 2012 博客原文
> "files share a same cid in a bt task, cid is the btih of the torrent"

### BCID 算法 (D 级, 完全未公开)

- GitHub 搜 `BCID` 无迅雷相关结果
- 所有内部 RTTI 类名 (`CalcBCIDTask`, `m_bcidInfo`, `XPF_HASHTYPE_BCID`) 公网零命中
- binux 2012 文档只有 CID/GCID,无 BCID

**推测**: BCID 是 2012 年后引入的 P2SP 块级哈希,算法未公开。

**重要**: BCID 对 BT 接续**非必需** (B 级),因为标准 BT 只需 piece SHA1 + piece length + bitfield。

---

## 8. XPF_HASHTYPE 枚举 (A 级, 反汇编 P2PBase.dll)

```
XPF_HASHTYPE_CID    = ?  (数值未确认)
XPF_HASHTYPE_BCID   = ?
XPF_HASHTYPE_GCID   = ?
XPF_HASHTYPE_URL    = ?
XPF_HASHTYPE_MD5    = ?
XPF_HASHTYPE_SHA1   = ?
```

字符串证据确认 6 种类型存在,但**具体数值未反汇编出来**。

---

## 9. 待验证清单

### P0 (阻塞转换器实现)

| # | 问题 | 当前等级 | 验证方法 |
|---|---|---|---|
| V1 | section_id → 内容映射 | D | 真实 .xlbt.cfg hex 验证 |
| V2 | field2/field3 语义 (size/offset?) | C | 真实 .xlbt.cfg + 解析器对照 |
| V3 | .bt.xltd 是否有头部 | B | 真实 .bt.xltd 前 64 字节 hex |
| V4 | piece 数据物理偏移公式 | C | 真实样本 + piece hash 比对 |
| V5 | CXBitmap 字节序 | D | 真实 cfg 中 BITFIELD section hex |
| V6 | CXBitmap 是否每 piece 1 bit | D | BITFIELD section size vs num_pieces |
| V7 | cfg info hash 校验算法 | C | 反汇编 BTTask::GetInfoHash 完整逻辑 |
| V8 | block_count / block_size 实际语义 | C | 真实 cfg 字段值范围分析 |

### P1 (影响兼容性,非阻塞)

| # | 问题 | 当前等级 | 验证方法 |
|---|---|---|---|
| V9 | XPF_HASHTYPE_* 具体数值 | C | 反汇编 GetHashTypeFromString |
| V10 | BCID 算法 | D | 反汇编 CalcBCIDTask |
| V11 | cid_store.dat 格式 | D | 真实文件 hex |

---

## 10. 验证流程

收到真实样本后,执行 `validate_xunlei_sample.py`:

1. 读 .torrent → 拿 piece_length, pieces_hash, info_hash, num_pieces
2. 读 .xlbt.cfg 头部 → 验证 magic / block_size 对齐
3. 读 section 数组 → 列出所有 section_id + field2 + field3
4. 对每个 section,尝试按推测的内容类型解析
5. **关键验证**: 从 .bt.xltd 抽取已下载 piece 数据,计算 SHA1,与 .torrent 的 pieces_hash 比对
   - 若命中率 ≥ 80% → 确认 .bt.xltd 偏移布局
   - 若命中率 < 80% → 偏移布局假设错误,需重新逆向
6. 输出验证报告,标记每个推测的验证结果

验证通过后,才能解锁转换器的"假设分支"。

---

## 11. 禁止事项

- ❌ 禁止将本规范中的任何 C/D 级字段写入生产代码
- ❌ 禁止假设 section 0 = INFO_HASH, section 1 = PIECES_HASH 等
- ❌ 禁止假设 .bt.xltd 偏移 = `piece_index × piece_length` (虽然推断是这样)
- ❌ 禁止在没运行 validate_xunlei_sample.py 的情况下启用转换器

## 12. 允许事项

- ✅ 可基于 A 级字段(magic, 头部布局, section entry 结构)写解析器
- ✅ 可基于 A 级字段做"诊断模式"输出
- ✅ 可基于 B 级推断做"假设分支",但默认禁用
- ✅ 可基于 A 级开源算法(GCID/CID/SHA1)做哈希计算

---

## 13. 变更日志

- 2026-08-16 初版,基于反汇编推断冻结
- 待真实样本验证后更新

## 最后更新

2026-08-16 15:50 UTC+8
