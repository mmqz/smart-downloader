# P2P 网络接入状态 - 最终

## 当前状态

**逆向到 capstone 反汇编极限，需要更深层工具或真实抓包样本。**

## 已确认 (A 级)

### PHub 包格式
```
[0:4]   cmd_id = 1 (uint32 LE)
[4]     flag = 0xb/0x11/0x13 (uint8)
[5:9]   sequence (uint32 LE, 递增)
[9:13]  enc_len = total_len - 13 (uint32 LE)
[13:]   AES-ECB(MD5([0:4]+[5:9]), body)
```

### AES key 派生
```
key = MD5(cmd_id_bytes(4) + seq_bytes(4))  → 16 字节 AES-128 key
加密: AES-128-ECB + PKCS7
范围: [13:] (前 13 字节不加密)
```

### RSA 公钥
4 个 RSA-1024 (e=65537),公钥 #2 用于 PHubQueryRes,公钥 #3 用于 AllRes

### 服务器
pr-phub.sandai.net:80/443 可达,POST / 返回 "decrypt request failed"

### captcha_sign
算法实现正确,沙箱能拿 captcha_token

## 未解决

15 个 PoC 全部被 PHub 拒绝。可能原因:
1. HTTP 发送函数 (0x1802833e0) 对加密包做了**额外包装**
2. AES key 不是从包数据派生,而是从**预共享密钥**
3. RSA 加密用了**自定义 padding** (XPF_RSAEncrypt_PKCS1_EX)
4. 需要**先调 ConfigHub** 拿到 Auth Key

## 阻碍

- capstone 只能反汇编,不能反编译 → 看不到 C 伪代码
- Ghidra 需要 Java + GhidraClassLoader,沙箱内存有限 (4GB),分析 4.7MB DLL 容易 OOM
- 沙箱无法跑真实 BT → 无抓包样本
- DNS 污染 → hub5p.sandai.net 不可达

## 下一步选项

1. **用户用 Wireshark 抓 pr-phub.sandai.net:80 的 HTTP POST body** — 在 Windows 上跑迅雷下载一个 BT 任务,用 Wireshark 过滤 `host pr-phub.sandai.net && http.request.method == POST`,导出 HTTP body 给我。1 个真实包就能破解整个格式。

2. **Ghidra 后台分析** — 用 `setsid nohup` 后台跑,等几分钟后读结果

3. **继续反汇编 HTTP 发送函数 0x1802833e0** — 找 HTTP body 构造逻辑
