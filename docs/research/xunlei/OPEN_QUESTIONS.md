# Open Questions (Round 4 后)

按优先级: **影响程度 × 不确定性 × 可验证性**

## P0 - 阻塞最终结论

### Q12 (修正). .xlbt.cfg 每个 section_id 对应什么内容?

**当前状态**: 部分回答
- ✅ 文件 magic 已破解 (A10: "XLBTCFG\x00")
- ✅ 头部 40 字节结构已破解 (A11)
- ✅ section entry 20 字节结构已破解 (A12)
- ❌ 每个 section_id 对应什么内容未确认
- ⚠ 需要真实文件样本做 hex 验证

### Q11 (修正). .bt.xltd 文件二进制结构?

**当前状态**: B 级 (强力推断)
- ✅ BTPureDataBlockReader 类存在 (F26)
- ✅ 'BT_PURE_DataBlock_Reader' 字符串引用 (F26)
- ✅ 无 ASCII magic (除 XLBTCFG 外没其他 movabs magic)
- ⚠ 推断为纯 piece 数据 sparse file (按偏移存储)
- ❌ 真实文件 hex 验证未做

### Q13 (旧). CXBitmap 类的二进制格式?

**当前状态**: D 级
- 字段: `bitmap_count` + `Bitmap_len`
- 内部布局未验证

### Q17 (新). 是否需要让用户提供真实样本?

**当前判断**: **是,需要**

理由:
- 反汇编已到极限,继续反汇编 ROI 低
- 真实 .xltd + .xlbt.cfg + .torrent 样本能立即验证:
  - .bt.xltd 是否纯数据 (Q11)
  - 每个 section 内容 (Q12)
  - CXBitmap 格式 (Q13)
- 这是"必须由用户提供文件"的情况,符合暂停条件 B

## P1 - 影响架构决策

### Q3 (合并到 Q11)
### Q4-Q6 (不影响路径 D)
### Q9 (旧) - 已完成迅雷 BT 数据能否被标准 BT 客户端验证?

**当前状态**: 部分回答
- ✅ 算法层: 标准 piece SHA1 可校验 (F9)
- ✅ 推断: .bt.xltd 是纯数据,可被 libtorrent 读取 (F26)
- ❌ 物理层验证: 需要真实样本测试

## 已关闭的问题

- Q10: 第三方是否已实现 .xltd 兼容? → 已验证不存在
- Q1 (部分): CID/GCID 算法 → 已通过开源资料验证
- H3: 迅雷不存储标准 BT piece hash → **被证伪**
- H7: 独立标准 piece 数据文件 → **被反证** (.bt.xltd 自身就是)
- H4: .xltd 即使含完整 piece 数据, 也无法被 libtorrent 接续 → **被部分证伪** (推断为纯数据可被读取)

## 新增问题

### Q18 (新). 是否需要写一个 PoC 转换器验证推断?

**目标**: 写一个最小 Rust 程序
1. 读 .xlbt.cfg → 解析 magic + 头部 → 列出所有 section
2. 读 .bt.xltd → 检查是否有头部 magic, 或直接是 piece 数据
3. 输出 hex dump + 字段解读

**工作量**: 0.5-1 天
**收益**: 验证所有反汇编推断,无需用户提供样本

## 下一轮研究顺序

1. **写 PoC 转换器** (验证推断) - 1 天
   - 输入: .xlbt.cfg 文件 (用户提供)
   - 输出: 解析报告 (magic / 头部字段 / section 列表)
2. 如果用户能提供样本 → 立即验证
3. 如果用户不能提供样本 → 让用户跑一个迅雷 BT 任务, 抓样本
4. 验证后,评估是否完成研究

## 最后更新时间

2026-08-16 14:50 UTC+8
