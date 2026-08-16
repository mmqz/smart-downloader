# 迅雷样本采集手册

> 目的: 在 Windows 上采集迅雷 BT 下载样本,用于验证逆向推断
> 预期耗时: 5-10 分钟(不含下载时间)
> 用户操作难度: 简单

---

## 为什么需要样本

我们在沙箱环境已通过反汇编推断出迅雷 .xlbt.cfg 和 .bt.xltd 的格式(见 `spec_pending_validation.md`),但**所有推断都未通过真实样本验证**。沙箱网络限制使我们无法跑真实 BT 任务,只能请你帮忙采集一份样本。

样本拿到后,**1 小时内**可把所有 C/D 级推断升级为 A 级,解锁转换器。

---

## 采集步骤

### 步骤 1: 准备迅雷 + 测试任务

1. 打开迅雷 PC 版(v25.x 即可,我们逆向的就是这个版本)
2. 找一个**小的磁力链接或 .torrent 文件**:
   - 推荐: 任何 100MB-1GB 的公开资源(如 Linux ISO 镜像、公开课程视频等)
   - **避免**: 极小文件(<10MB)或极大文件(>5GB)
3. 把磁力或 .torrent 拖入迅雷开始下载
4. **下载到 30-50%** 时停止(不要等下完)
   - 这样能产生有 piece 数据 + bitfield 的真实 .xltd/.cfg

### 步骤 2: 完全退出迅雷(关键)

⚠ **关键**: 迅雷运行时**会锁定 .bt.xltd 和 .xlbt.cfg 文件**,无法复制。

完全退出迅雷的方法:
1. 右键托盘区迅雷图标 → 退出
2. 任务管理器(Ctrl+Shift+Esc)→ 详细信息 → 找 `XLUE.exe`、`Thunder.exe`、`DownloadSDKServer.exe` 等进程,**全部结束**
3. 等待 5 秒确保文件句柄释放

### 步骤 3: 找到样本文件

迅雷默认下载目录:
```
C:\Users\<你的用户名>\Downloads\
或
D:\迅雷下载\
```

打开下载目录,你会看到:

```
<下载目录>\
├─ <任务名>/                          ← 子目录(多文件种子) 或
├─ <任务名>                           ← 单文件
├─ <任务名>.bt.xltd       ★ 必需    ← 迅雷 BT 临时数据
├─ <任务名>.xlbt.cfg      ★ 必需    ← 迅雷 BT 任务配置
├─ <任务名>.xlbt.dat      (可选)    ← BT 任务数据
└─ (其他 .td / .cfg 等旧格式文件)
```

**关键文件**(必需):
1. `<任务名>.bt.xltd` — BT 临时数据
2. `<任务名>.xlbt.cfg` — BT 任务配置
3. **原始 .torrent 文件**(如果你用的是磁力,需要从迅雷里导出,见下方"导出 .torrent")

### 步骤 4: (仅磁力任务需要) 导出 .torrent 文件

如果你用的是磁力链接,迅雷已经下载了 metadata,但默认不保存为 .torrent 文件。

导出方法:
1. 在迅雷任务列表右键 → "打开文件夹"
2. 在下载目录找 `<任务名>.bt.xltd` 同目录下是否已有 .torrent 文件
3. 若没有,在迅雷设置里:
   - 设置 → 高级设置 → BT 任务设置 → 勾选"下载完成后保留种子文件"
   - 或在迅雷里右键任务 → "另存为种子文件"

如果以上都做不到,请告诉我磁力链接,我用 libtorrent 重新拿 metadata 后给你一个 .torrent。

### 步骤 5: 复制三件套到一个新目录

新建目录,例如:
```
C:\Users\<你>\Desktop\xunlei_sample\
```

复制以下文件进去:
1. `<任务名>.bt.xltd`
2. `<任务名>.xlbt.cfg`
3. `<原始.torrent>`(可能名为 `<任务名>.torrent` 或保留原文件名)
4. (可选) `<任务名>.xlbt.dat`
5. (可选) `cid_store.dat`(在 `C:\Users\<你>\AppData\Roaming\Thunder Network\` 或类似路径)

### 步骤 6: 打包 + 上传

把整个 `xunlei_sample\` 目录打包成 zip:
- 右键 → 发送到 → 压缩(zipped)文件夹
- 或用 7-Zip 打包

**压缩后预期大小**: 远小于下载文件本身(因为 .bt.xltd 是 sparse 文件,压缩后会很小)
- 100MB 下载任务 → zip 可能只有 1-10MB

把 zip 文件提供给我。

---

## 检查清单(打包前自查)

请打包前确认:

### 文件清单
- [ ] `<任务名>.bt.xltd` 存在,且大小 > 0
- [ ] `<任务名>.xlbt.cfg` 存在,且大小 > 40 字节(应至少几百字节)
- [ ] 原始 `.torrent` 文件存在,且大小 > 100 字节
- [ ] (可选) `<任务名>.xlbt.dat` 存在
- [ ] (可选) `cid_store.dat` 存在

### 文件大小合理性
- `.bt.xltd` 大小应**等于下载任务的完整大小**(sparse 文件,size = total_size,但实际占用 < size)
  - 在 Windows 资源管理器里看大小: 显示"大小"和"占用空间"两个值
  - **"大小"应等于下载文件总大小**(如 1GB)
  - **"占用空间"应远小于"大小"**(如只下了 30%,占用空间约 300MB)
- `.xlbt.cfg` 大小应在几百字节到几 KB 之间
- `.torrent` 大小应在几 KB 到几 MB 之间

### 时间戳
- 三个文件的时间戳应相近(都是任务暂停时的时间)
- 时间戳应在你的"暂停迅雷"动作前几秒内

### 隐私检查(重要)
- [ ] `.torrent` 文件不包含个人隐私(种子文件本身没有用户信息)
- [ ] `.xlbt.cfg` 内**可能**包含你的迅雷 ID 或 device_id,如有顾虑请先用十六进制编辑器检查前 100 字节,删除可疑字段
- [ ] `cid_store.dat` **强烈建议不提供** — 它包含你所有迅雷下载历史,有强隐私性
- [ ] `.bt.xltd` 内只有 piece 数据,无隐私

---

## 不需要做的事情

- ❌ 不需要修改任何文件
- ❌ 不需要导出 .torrent 到特定格式
- ❌ 不需要安装额外工具
- ❌ 不需要在迅雷里做特殊设置
- ❌ 不需要等下载完成

---

## 样本将用于什么

收到样本后,我会:

1. 运行 `validate_xunlei_sample.py` 验证所有推断(约 5 秒)
   - 验证 .xlbt.cfg magic = "XLBTCFG"
   - 验证 section_id → 内容映射
   - 验证 .bt.xltd 是否纯数据 sparse
   - 验证 piece 偏移公式 = piece_index × piece_length
   - 验证 CXBitmap 字节序
   - 验证 cfg 内 infohash 与 .torrent 一致

2. 如果验证通过 → 升级 `spec_pending_validation.md` 中所有 C/D 级为 A 级

3. 运行 `xunlei_to_libtorrent_converter.py --convert` 实际转换
   - 输入: 你的三件套
   - 输出: libtorrent fastresume + .part 文件
   - 你可拿 .part + .torrent 在 qBittorrent 里验证(可选,确认转换正确)

4. 把验证报告 + 转换结果反馈给你

---

## 如果采集遇到问题

### 问题 1: 找不到 .bt.xltd 文件
- 检查: 是否真的是 BT 任务(不是 HTTP/FTP 任务)
- HTTP/FTP 任务用 `.td` + `.td.cfg` 格式,不是 BT 的 `.bt.xltd` + `.xlbt.cfg`
- 解决: 找一个真正的 .torrent 或磁力任务

### 问题 2: 文件被锁定无法复制
- 迅雷没完全退出
- 解决: 任务管理器结束所有迅雷进程(XLUE.exe / Thunder.exe / DownloadSDKServer.exe)

### 问题 3: 没有原始 .torrent 文件
- 你用的是磁力链接
- 解决: 见步骤 4 "导出 .torrent 文件"

### 问题 4: 文件太大无法上传
- .bt.xltd 即使是 sparse 文件,zip 压缩后应该很小
- 如果还太大,只提供 `.xlbt.cfg` + `.torrent` 也可以(我能验证大部分推断,但不能验证 .bt.xltd 偏移)
- 极端情况: 让我提供给你一个我生成的测试磁力链接,你下个 30% 给我

---

## 联系方式

提供样本后,告诉我:
- 你的迅雷版本(从 "帮助 → 关于" 看)
- 任务名 + 文件大小
- 下载进度百分比

我会立即开始验证,通常 1 小时内反馈结果。

---

## 附: 我会用的验证命令

收到样本后我会跑:

```bash
# 1. 验证 (仓库内路径, 在 E:\Code\ai\smart-downloader 下执行)
python tools/xunlei-migrate/validate_xunlei_sample.py \
  --torrent your_sample/task.torrent \
  --bt-xltd your_sample/task.bt.xltd \
  --cfg your_sample/task.xlbt.cfg \
  --report your_sample/verification.json

# 2. 转换(验证通过后)
python tools/xunlei-migrate/xunlei_to_libtorrent_converter.py \
  --torrent your_sample/task.torrent \
  --bt-xltd your_sample/task.bt.xltd \
  --cfg your_sample/task.xlbt.cfg \
  --output-dir your_sample/output \
  --convert
```

输出会包含:
- `verification.json` — 完整验证报告
- `output/task.fastresume` — libtorrent fastresume
- `output/task.part` — 标准 .part 文件
- `output/conversion_report.json` — 转换结果

你可以拿 fastresume + .part + 原始 .torrent,在 qBittorrent 里:
- 添加 .torrent 文件
- 选 .part 所在目录
- qBittorrent 会自动 rehash,已下载的 piece 不会重传
