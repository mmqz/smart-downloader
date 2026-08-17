//! thunder:// 解码（§7.1 D36）。
//! thunder:// 内容 = base64("AA" + 真实URL + "ZZ")。

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ThunderError {
    #[error("missing thunder:// prefix")]
    MissingPrefix,
    #[error("invalid base64: {0}")]
    InvalidBase64(#[from] base64::DecodeError),
    #[error("missing AA/ZZ shell")]
    MissingShell,
}

/// 解码 thunder:// 链接 → 真实 URL（≤30 行实现）。
pub fn decode_thunder(link: &str) -> Result<String, ThunderError> {
    let rest = link
        .strip_prefix("thunder://")
        .ok_or(ThunderError::MissingPrefix)?;
    let decoded = B64.decode(rest.as_bytes())?;
    let s = String::from_utf8_lossy(&decoded);
    let inner = s
        .strip_prefix("AA")
        .and_then(|x| x.strip_suffix("ZZ"))
        .ok_or(ThunderError::MissingShell)?;
    if inner.is_empty() {
        return Err(ThunderError::MissingShell);
    }
    Ok(inner.to_string())
}