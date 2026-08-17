# 真实样本 samples/

已验证的真实迅雷样本 (2026-08-17, 任务 audio-books-cjk, infohash
`C5AA149AE0776344A270EAFEE49FDADB43FF6097`, 2263 pieces @ 131072, ~83% 下载):

| 文件 | 大小 | 说明 |
|---|---|---|
| `audio-books-cjk.torrent` | 47 KB | 原始 .torrent (4 文件: cover.jpg + metadata.json + metadata.opf + 296MB m4b) |
| `C5AA149AE0776344A270EAFEE49FDADB43FF6097.xlbt.cfg` | 32 KB | 迅雷任务配置 (真实格式, 见 spec_pending_validation.md) |
| `cover.jpg.bt.xltd` | 741 KB | 文件 0 (cover.jpg) 的位置镜像样本 |

**未入库的大文件** (保留在原始目录, 供 V4 全量验证):
- `...\My Girlfriend...v01 [Seven Seas Siren] [Stick].m4b.bt.xltd` (295,849,984 B)
- 同目录 `.m4b` 数据文件 (295,849,204 B, 迅雷已物化)

## 复跑验证

```powershell
# 8 项验证 (V1-V8, 用 samples 内小样本 + 原始目录大样本)
cd tools\xunlei-migrate
$env:PYTHONIOENCODING='utf-8'
python validate_xunlei_sample.py `
  --torrent "samples\audio-books-cjk.torrent" `
  --cfg "samples\C5AA149AE0776344A270EAFEE49FDADB43FF6097.xlbt.cfg" `
  --xltd-dir "E:\迅雷下载\云盘下载\audio-books-cjk" `
  --report "samples\validation_report.json"

# cfg 结构解析
python parse_xlbt_cfg.py "samples\C5AA149AE0776344A270EAFEE49FDADB43FF6097.xlbt.cfg"

# 转换器诊断 (m4b.xltd 在原始目录)
python xunlei_to_libtorrent_converter.py `
  --torrent "samples\audio-books-cjk.torrent" `
  --cfg "samples\C5AA149AE0776344A270EAFEE49FDADB43FF6097.xlbt.cfg" `
  --xltd-dir "E:\迅雷下载\云盘下载\audio-books-cjk" `
  --output-dir "output\audio-books-cjk"

# 合成 e2e (自包含, 无需真实样本)
python e2e_test_converter.py
```

## 注意事项

- 样本来自用户自己的迅雷任务 (个人使用); 任务当前仍在下载, piece 数会随时间变化
- `.bt.xltd` 是文件位置镜像 (无头, 4096 对齐, 零区 = 未下载), 不是 piece 平铺
- cfg 是任务元数据 (peer 缓存/统计), **无 piece 哈希/位图**; 完成状态只能由
  xltd 数据 + torrent 哈希 SHA1 推导