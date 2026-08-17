//! ContentIdentity 校验（§14 Q-B5）：sha256 仅用户/源提供时启用；
//! 失败重下 1 次后降级接受（告警）。ETag+Content-Length 为准（校验在段层完成）。

use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// 计算文件 SHA256（hex）。
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(format!("{:x}", h.finalize()))
}

/// 校验文件内容与期望 SHA256 是否一致。文件缺失 → Err。
pub fn verify_file(path: &Path, expected_hex: &str) -> io::Result<bool> {
    Ok(sha256_file(path)? == expected_hex)
}
