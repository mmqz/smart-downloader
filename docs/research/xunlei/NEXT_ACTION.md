# Next Action - 真实样本验证已完成

## 状态: 转换器路径已验证, 可进入产品化

### 2026-08-17 里程碑
- ✅ 真实样本三件套到位 + 验证 V1-V8 全绿 (validate_xunlei_sample.py)
- ✅ 3 项核心反汇编推断被真实样本推翻并修正 (magic / section 数组 / bitfield)
- ✅ 转换器重构为真实格式, e2e 通过 (fastresume + 位图 + 物化)
- ✅ spec_pending_validation.md 升级为 A 级 (已验证版)

## 待办 (按优先级)

### P0: 转换器产品化 (对接 M 系列)
- [ ] 真实任务端到端试跑: 用 audio-books-cjk 样本生成 fastresume,
      在 qBittorrent 加载验证 rehash 行为 (用户机器上有迅雷环境)
- [ ] 与主项目集成: btcore 的"导入迅雷任务"入口 (M3+ 范畴)
- [ ] 处理边界: 在途 piece (partial) 策略确认 = 视为未完成 (已实现)

### P1: 格式补全 (B/C 级遗留, 不影响转换)
- [ ] cfg 头部 0x08-0x3B 字段语义 (需要更多样本对照)
- [ ] tag-02 key 2..2200 / 64KB 块记录 / 231 个 20B blob 语义
- [ ] peer 缓存记录内部字段 (可用于转换后 peer 注入)

### P2: 样本扩充 (可选)
- [ ] 收集第二个不同任务样本 (不同 piece_length / 单文件种子), 验证公式通用性
      (用户可把新样本丢进 tools/xunlei-migrate/samples/ 重跑验证器)

## 用户配合事项

- 保持 audio-books-cjk 任务完成下载后, 可复跑验证器确认"全量分配 + 尾部零区"最终态
- 若要试 qBittorrent 迁移: 转换器输出 fastresume + 数据文件 → 添加到 qBittorrent 验证
