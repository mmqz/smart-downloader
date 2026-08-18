//! thunder:// 解码（§7.1 D36）。
//! thunder:// 内容 = base64("AA" + 真实URL + "ZZ")。
//!
//! P0 增强（thunder-https 分析落地）：容错解码——
//! - base64 padding 缺失自动补全（老链接常被截断 `=`）
//! - 无 AA/ZZ 壳兜底（早期工具直接 base64(url)）

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

/// 容错 base64 解码：自动补全 padding（thunder:// 链接截断常见）。
/// 规则：长度 %4=2 补 `==`，%4=3 补 `=`；%4=1 为非法（由 base64 库拒绝）。
pub fn decode_base64_lenient(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let trimmed = input.trim();
    let mut padded = trimmed.to_string();
    while !padded.len().is_multiple_of(4) {
        padded.push('=');
    }
    B64.decode(padded.as_bytes())
}

/// 解码 thunder:// 链接 → 真实 URL。
/// 优先 AA/ZZ 标准壳；失败时尝试无壳直链（base64(url) 老格式）。
pub fn decode_thunder(link: &str) -> Result<String, ThunderError> {
    let rest = link
        .strip_prefix("thunder://")
        .ok_or(ThunderError::MissingPrefix)?;
    let decoded = decode_base64_lenient(rest)?;
    let s = String::from_utf8_lossy(&decoded);
    // 1) 标准壳 AA...ZZ
    if let Some(inner) = s.strip_prefix("AA").and_then(|x| x.strip_suffix("ZZ")) {
        if !inner.is_empty() {
            return Ok(inner.to_string());
        }
    }
    // 2) 无壳兜底：内容本身是 http 链接（老格式直接 base64(url)）
    if looks_like_url(&s) {
        return Ok(s.to_string());
    }
    Err(ThunderError::MissingShell)
}

fn looks_like_url(s: &str) -> bool {
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("magnet:")
        || s.starts_with("ftp://")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc_b64(s: &str) -> String {
        B64.encode(s.as_bytes())
    }

    fn strip_padding(s: &str) -> String {
        s.trim_end_matches('=').to_string()
    }

    #[test]
    fn standard_shell_decodes() {
        let link = format!("thunder://{}", enc_b64("AAhttp://example.com/a.binZZ"));
        assert_eq!(decode_thunder(&link).unwrap(), "http://example.com/a.bin");
    }

    #[test]
    fn missing_padding_fixed() {
        // 剥掉 padding（截断的老链接）→ 自动补全后仍解码
        let full = enc_b64("AAhttp://example.com/a.binZZ");
        let cut = strip_padding(&full);
        assert_ne!(cut, full, "确保 padding 确实被剥掉");
        let link = format!("thunder://{cut}");
        assert_eq!(decode_thunder(&link).unwrap(), "http://example.com/a.bin");
    }

    #[test]
    fn partial_padding_fixed() {
        // 长度 %4=3 → 补一个 =
        let full = enc_b64("AAhttp://example.com/a.binZZ");
        let one_cut = full.trim_end_matches("==").to_string();
        let link = format!("thunder://{one_cut}");
        assert_eq!(decode_thunder(&link).unwrap(), "http://example.com/a.bin");
    }

    #[test]
    fn no_shell_direct_url_fallback() {
        // 无 AA/ZZ 壳：直接 base64(url) 老格式
        let link = format!("thunder://{}", enc_b64("http://example.com/old.bin"));
        assert_eq!(decode_thunder(&link).unwrap(), "http://example.com/old.bin");
    }

    #[test]
    fn bad_base64_rejected() {
        assert!(matches!(
            decode_thunder("thunder://!!!not-base64!!!"),
            Err(ThunderError::InvalidBase64(_))
        ));
    }

    #[test]
    fn garbage_with_shell_missing_rejected() {
        let link = format!("thunder://{}", enc_b64("not-a-url-at-all"));
        assert_eq!(decode_thunder(&link), Err(ThunderError::MissingShell));
    }

    #[test]
    fn missing_prefix_rejected() {
        assert_eq!(decode_thunder("http://x"), Err(ThunderError::MissingPrefix));
    }

    #[test]
    fn lenient_base64_qqdl_style() {
        // qqdl 分支复用：无壳 base64 直接解码
        let raw = enc_b64("http://example.com/q.bin");
        assert_eq!(
            String::from_utf8(decode_base64_lenient(&raw).unwrap()).unwrap(),
            "http://example.com/q.bin"
        );
    }
}
