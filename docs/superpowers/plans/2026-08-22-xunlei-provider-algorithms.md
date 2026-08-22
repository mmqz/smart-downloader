# 迅雷云盘 Provider — 算法模块实现计划（第一期：纯算法）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现迅雷云盘 Provider 的纯算法地基（captcha_sign / device_sign / GCID / CID），为后续 HTTP 客户端 + 登录 + 取链打基础。

**Architecture:** 在 `crates/provider/src/xunlei/` 下新建算法模块，纯函数、无 I/O、无 async，全部可单元测试。算法从 alist（MIT）Go 源码移植，用 RustCrypto 的 `md-5` + `sha1` 实现。

**Tech Stack:** Rust 2021、md-5 0.10、sha1 0.10、hex 编码（手写，避免额外依赖）

---

## 背景（写给零上下文工程师）

迅雷云盘（pan.xunlei.com）的 API 需要几个签名算法，全部是公开算法（来自开源项目 alist / xunlei-lixian，MIT 许可）：

1. **captcha_sign**：`"1." + 多轮 md5`，用于 captcha/init 接口
2. **device_sign**：`"div101." + device_id + md5(sha1(base))`，用于登录
3. **GCID**：文件内容哈希，piece 大小动态增长
4. **CID**：3×20KB 采样 SHA1（文件 <60KB 则全文）

这些算法是**纯函数**，不依赖网络、不依赖迅雷客户端，可以完全本地测试。

### 已知常量（来自 alist，写死在代码里）

```rust
// captcha_sign 的 10 个盐（alist 开源，MIT）
const ALGORITHMS: [&str; 10] = [
    "9uJNVj/wLmdwKrJaVj/omlQ",
    "Oz64Lp0GigmChHMf/6TNfxx7O9PyopcczMsnf",
    "Eb+L7Ce+Ej48u",
    "jKY0",
    "ASr0zCl6v8W4aidjPK5KHd1Lq3t+vBFf41dqv5+fnOd",
    "wQlozdg6r1qxh0eRmt3QgNXOvSZO6q/GXK",
    "gmirk+ciAvIgA/cxUUCema47jr/YToixTT+Q6O",
    "5IiCoM9B1/788ntB",
    "P07JH0h6qoM6TSUAK2aL9T5s2QBVeY9JWvalf",
    "+oK0AN",
];

// 客户端身份
const CLIENT_ID: &str = "Xp6vsxz_7IYVw2BB";
const CLIENT_VERSION: &str = "8.31.0.9726";
const PACKAGE_NAME: &str = "com.xunlei.downloadprovider";

// device_sign 用的 APPID/APPKey
const APPID: &str = "40";
const APPKEY: &str = "34a062aaa22f906fca4fefe9fb3a3021";
```

---

## 文件结构

```
crates/provider/src/xunlei/
├── mod.rs        # 模块声明 + re-export
├── sign.rs       # captcha_sign + device_sign
├── hash.rs       # GCID + CID（文件内容哈希）
└── (后续: client.rs, share.rs, link.rs —— 不在本期)
```

- `sign.rs`：签名算法，输入字符串，输出签名字符串，纯函数。
- `hash.rs`：文件哈希，输入 `&[u8]` 或 `Read`，输出 40 位 hex。

---

## 依赖变更

`crates/provider/Cargo.toml` 增加：

```toml
md-5 = "0.10"
```

> `sha1` 已在 workspace 依赖（`sha1 = "0.10"`），但 provider 的 Cargo.toml 当前**没有**声明 `sha1`。需要在 provider/Cargo.toml 加 `sha1 = { workspace = true }`。

---

## Task 1: 新建模块骨架 + captcha_sign

**Files:**
- Create: `crates/provider/src/xunlei/mod.rs`
- Create: `crates/provider/src/xunlei/sign.rs`
- Modify: `crates/provider/src/lib.rs`（加 `pub mod xunlei;`）
- Modify: `crates/provider/Cargo.toml`（加 `md-5` 和 `sha1`）

- [ ] **Step 1: 在 Cargo.toml 加依赖**

在 `crates/provider/Cargo.toml` 的 `[dependencies]` 段加：

```toml
md-5 = "0.10"
sha1 = { workspace = true }
```

- [ ] **Step 2: 写失败测试（captcha_sign）**

创建 `crates/provider/src/xunlei/sign.rs`，先写测试（内联 `#[cfg(test)]`）：

```rust
//! 迅雷云盘签名算法（captcha_sign / device_sign），纯函数。

/// 手写 hex 编码（避免引入 hex crate）。
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encodes_correctly() {
        assert_eq!(to_hex(&[0x00, 0xff, 0x0a]), "00ff0a");
    }

    #[test]
    fn captcha_sign_starts_with_1_dot() {
        let sign = captcha_sign("device123", "1700000000000");
        assert!(sign.starts_with("1."));
    }

    #[test]
    fn captcha_sign_is_32_hex_after_prefix() {
        let sign = captcha_sign("device123", "1700000000000");
        let hex_part = sign.strip_prefix("1.").unwrap();
        assert_eq!(hex_part.len(), 32);
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn captcha_sign_is_deterministic() {
        let a = captcha_sign("dev", "1000");
        let b = captcha_sign("dev", "1000");
        assert_eq!(a, b);
    }

    #[test]
    fn captcha_sign_changes_with_timestamp() {
        let a = captcha_sign("dev", "1000");
        let b = captcha_sign("dev", "1001");
        assert_ne!(a, b);
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p smart-dl-provider sign::`
Expected: 编译失败，`captcha_sign` 未定义。

- [ ] **Step 4: 实现 captcha_sign**

在 `sign.rs` 加实现（用 `md5` crate）：

```rust
use md5::Md5;
use md5::Digest;

/// captcha_sign 的 10 个盐（alist 开源，MIT）。
const ALGORITHMS: [&str; 10] = [
    "9uJNVj/wLmdwKrJaVj/omlQ",
    "Oz64Lp0GigmChHMf/6TNfxx7O9PyopcczMsnf",
    "Eb+L7Ce+Ej48u",
    "jKY0",
    "ASr0zCl6v8W4aidjPK5KHd1Lq3t+vBFf41dqv5+fnOd",
    "wQlozdg6r1qxh0eRmt3QgNXOvSZO6q/GXK",
    "gmirk+ciAvIgA/cxUUCema47jr/YToixTT+Q6O",
    "5IiCoM9B1/788ntB",
    "P07JH0h6qoM6TSUAK2aL9T5s2QBVeY9JWvalf",
    "+oK0AN",
];

const CLIENT_ID: &str = "Xp6vsxz_7IYVw2BB";
const CLIENT_VERSION: &str = "8.31.0.9726";
const PACKAGE_NAME: &str = "com.xunlei.downloadprovider";

/// 计算 captcha_sign：
///   s = ClientID + ClientVersion + PackageName + DeviceID + timestamp
///   for salt in ALGORITHMS: s = md5(s + salt)
///   返回 "1." + s
pub fn captcha_sign(device_id: &str, timestamp_millis: &str) -> String {
    let mut s = format!(
        "{}{}{}{}{}",
        CLIENT_ID, CLIENT_VERSION, PACKAGE_NAME, device_id, timestamp_millis
    );
    for salt in ALGORITHMS {
        let mut h = Md5::new();
        h.update(s.as_bytes());
        h.update(salt.as_bytes());
        s = to_hex(&h.finalize());
    }
    format!("1.{}", s)
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p smart-dl-provider sign::`
Expected: PASS（5 个测试全过）。

- [ ] **Step 6: Commit**

```bash
git add crates/provider/src/xunlei/ crates/provider/src/lib.rs crates/provider/Cargo.toml
git commit -m "feat(provider): xunlei sign module — captcha_sign 纯算法 + hex"
```

---

## Task 2: device_sign

**Files:**
- Modify: `crates/provider/src/xunlei/sign.rs`

- [ ] **Step 1: 写失败测试**

在 `sign.rs` 的 `tests` mod 加：

```rust
    #[test]
    fn device_sign_has_div101_prefix() {
        let s = device_sign("device123");
        assert!(s.starts_with("div101.device123"));
    }

    #[test]
    fn device_sign_is_deterministic() {
        assert_eq!(device_sign("dev"), device_sign("dev"));
    }

    #[test]
    fn device_sign_differs_by_device() {
        assert_ne!(device_sign("a"), device_sign("b"));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p smart-dl-provider sign::device_sign`
Expected: 编译失败，`device_sign` 未定义。

- [ ] **Step 3: 实现 device_sign**

在 `sign.rs` 加（用 `sha1` + `md5`）：

```rust
use sha1::Sha1;

const APPID: &str = "40";
const APPKEY: &str = "34a062aaa22f906fca4fefe9fb3a3021";

/// device_sign = "div101." + deviceID + md5_hex(sha1_hex(deviceID+packageName+APPID+APPKey))
pub fn device_sign(device_id: &str) -> String {
    let base = format!("{}{}{}{}", device_id, PACKAGE_NAME, APPID, APPKEY);
    let sha1_hex = {
        let mut h = Sha1::new();
        h.update(base.as_bytes());
        to_hex(&h.finalize())
    };
    let md5_hex = {
        let mut h = Md5::new();
        h.update(sha1_hex.as_bytes());
        to_hex(&h.finalize())
    };
    format!("div101.{}{}", device_id, md5_hex)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p smart-dl-provider sign::device_sign`
Expected: PASS（3 个测试）。

- [ ] **Step 5: Commit**

```bash
git add crates/provider/src/xunlei/sign.rs
git commit -m "feat(provider): xunlei device_sign 算法"
```

---

## Task 3: GCID（文件内容哈希）

**Files:**
- Create: `crates/provider/src/xunlei/hash.rs`
- Modify: `crates/provider/src/xunlei/mod.rs`

- [ ] **Step 1: 写失败测试**

创建 `hash.rs`：

```rust
//! 迅雷 GCID / CID 文件内容哈希（xunlei-lixian 公开算法）。

use sha1::Sha1;
use sha1::Digest;

/// GCID 的 piece 大小：0x40000 起，file_size/piece_size > 0x200 时翻倍，上限 0x200000。
fn calc_block_size(file_size: u64) -> u64 {
    let mut psize: u64 = 0x40000;
    while file_size / psize > 0x200 && psize < 0x200000 {
        psize <<= 1;
    }
    psize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_size_small_file_is_256k() {
        // 小文件（< 128MB）piece = 256KB
        assert_eq!(calc_block_size(1024 * 1024), 0x40000);
    }

    #[test]
    fn block_size_caps_at_2m() {
        // 超大文件 piece 上限 2MB
        assert_eq!(calc_block_size(100 * 1024 * 1024 * 1024), 0x200000);
    }

    #[test]
    fn gcid_is_40_hex() {
        let data = vec![0xAAu8; 1024 * 1024];
        let gcid = gcid(&data);
        assert_eq!(gcid.len(), 40);
        assert!(gcid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn gcid_is_deterministic() {
        let data = vec![0xBBu8; 4096];
        assert_eq!(gcid(&data), gcid(&data));
    }

    #[test]
    fn gcid_empty_data() {
        // 空文件：hash1 无输入 → SHA1(空) = da39a3ee5e6b4b0d3255bfef95601890afd80709
        assert_eq!(
            gcid(&[]),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p smart-dl-provider hash::`
Expected: 编译失败，`gcid` 未定义。

- [ ] **Step 3: 实现 GCID**

在 `hash.rs` 加：

```rust
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// GCID = SHA1( SHA1(piece1) || SHA1(piece2) || ... ) 的 hex。
/// piece 大小由 calc_block_size 动态决定。
pub fn gcid(data: &[u8]) -> String {
    let mut hash1 = Sha1::new();
    let psize = calc_block_size(data.len() as u64) as usize;
    for chunk in data.chunks(psize) {
        hash1.update(Sha1::digest(chunk));
    }
    to_hex(&hash1.finalize())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p smart-dl-provider hash::`
Expected: PASS（5 个测试）。

- [ ] **Step 5: Commit**

```bash
git add crates/provider/src/xunlei/hash.rs crates/provider/src/xunlei/mod.rs
git commit -m "feat(provider): xunlei GCID 文件哈希算法"
```

---

## Task 4: CID（3×20KB 采样 SHA1）

**Files:**
- Modify: `crates/provider/src/xunlei/hash.rs`

- [ ] **Step 1: 写失败测试**

在 `hash.rs` 的 `tests` mod 加：

```rust
    #[test]
    fn cid_small_file_is_full_sha1() {
        // 文件 < 60KB → SHA1 全文
        let data = vec![0x11u8; 1000];
        assert_eq!(cid(&data), to_hex(&Sha1::digest(&data)));
    }

    #[test]
    fn cid_large_file_samples_three_regions() {
        // 文件 ≥ 60KB → SHA1(头20KB || 1/3处20KB || 尾20KB)
        let data = vec![0x22u8; 100 * 1024]; // 100KB
        let c = cid(&data);
        assert_eq!(c.len(), 40);

        // 手工构造期望值
        let head = &data[0..20 * 1024];
        let mid_start = data.len() / 3;
        let mid = &data[mid_start..mid_start + 20 * 1024];
        let tail = &data[data.len() - 20 * 1024..];
        let mut h = Sha1::new();
        h.update(head);
        h.update(mid);
        h.update(tail);
        assert_eq!(c, to_hex(&h.finalize()));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p smart-dl-provider hash::cid`
Expected: 编译失败，`cid` 未定义。

- [ ] **Step 3: 实现 CID**

在 `hash.rs` 加：

```rust
/// CID (=DCID)：文件 <60KB → SHA1(全文)；否则 SHA1(头20KB || 1/3处20KB || 尾20KB)。
pub fn cid(data: &[u8]) -> String {
    if data.len() < 60 * 1024 {
        return to_hex(&Sha1::digest(data));
    }
    let head = &data[0..20 * 1024];
    let mid_start = data.len() / 3;
    let mid = &data[mid_start..mid_start + 20 * 1024];
    let tail = &data[data.len() - 20 * 1024..];
    let mut h = Sha1::new();
    h.update(head);
    h.update(mid);
    h.update(tail);
    to_hex(&h.finalize())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p smart-dl-provider hash::cid`
Expected: PASS（2 个测试）。

- [ ] **Step 5: Commit**

```bash
git add crates/provider/src/xunlei/hash.rs
git commit -m "feat(provider): xunlei CID 采样哈希算法"
```

---

## Task 5: mod.rs 组装 + re-export + 全量测试

**Files:**
- Create: `crates/provider/src/xunlei/mod.rs`

- [ ] **Step 1: 写 mod.rs**

```rust
//! 迅雷云盘（pan.xunlei.com）Provider 的算法地基：
//! captcha_sign / device_sign（sign.rs）、GCID / CID（hash.rs）。
//! 纯函数，无 I/O，算法移植自 alist（MIT）与 xunlei-lixian（公开）。

pub mod hash;
pub mod sign;

pub use hash::{cid, gcid};
pub use sign::{captcha_sign, device_sign};
```

- [ ] **Step 2: 在 lib.rs 声明模块**

在 `crates/provider/src/lib.rs` 的 `pub mod` 区加一行：

```rust
pub mod xunlei;
```

- [ ] **Step 3: 全量测试**

Run: `cargo test -p smart-dl-provider`
Expected: 全部 PASS（含既有 mock/coordinator 测试 + 新增 sign/hash 测试）。

- [ ] **Step 4: Commit**

```bash
git add crates/provider/src/xunlei/mod.rs crates/provider/src/lib.rs
git commit -m "feat(provider): xunlei 模块组装 + re-export"
```

---

## Self-Review 检查结果

1. **Spec coverage**：captcha_sign（Task1）、device_sign（Task2）、GCID（Task3）、CID（Task4）、模块组装（Task5）—— 覆盖了"算法模块"全部目标。
2. **Placeholder scan**：无 TBD/TODO，所有代码完整给出，测试有具体断言。
3. **Type consistency**：`captcha_sign(device_id: &str, timestamp: &str) -> String`、`device_sign(device_id: &str) -> String`、`gcid(data: &[u8]) -> String`、`cid(data: &[u8]) -> String`，签名前后一致；`to_hex` 在 sign.rs 和 hash.rs 各定义一次（各自私有，避免跨模块依赖）。

## 遗留（不在本期）

- HTTP 客户端 + 登录（扫码/creditkey/token）—— 等子 agent 验证 API 当前有效性后写
- 分享链接解析 + 取链 + 离线下载 —— 同上
- 这些算法**是否仍被迅雷接受**（盐值是否过时）—— 需实际调 captcha/init 验证
