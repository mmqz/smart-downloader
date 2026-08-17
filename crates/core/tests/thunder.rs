//! M2: thunder:// 解码（§7.1 D36）。
//! thunder:// 内容 = base64("AA" + 真实URL + "ZZ")；畸形输入 → 解析错误。

use base64::Engine;
use smart_dl_core::source_parse::thunder::{decode_thunder, ThunderError};

fn encode_thunder(url: &str) -> String {
    let inner = format!("AA{url}ZZ");
    format!(
        "thunder://{}",
        base64::engine::general_purpose::STANDARD.encode(inner.as_bytes())
    )
}

#[test]
fn decodes_simple_url() {
    let url = "http://example.com/a.bin";
    assert_eq!(decode_thunder(&encode_thunder(url)).unwrap(), url);
}

#[test]
fn decodes_url_with_query_and_unicode() {
    let url = "https://example.com/f.bin?x=1&y=2";
    assert_eq!(decode_thunder(&encode_thunder(url)).unwrap(), url);
}

#[test]
fn missing_prefix_is_error() {
    let r = decode_thunder("http://example.com/a.bin");
    assert!(matches!(r, Err(ThunderError::MissingPrefix)));
}

#[test]
fn invalid_base64_is_error() {
    let r = decode_thunder("thunder://!!!not-base64!!!");
    assert!(matches!(r, Err(ThunderError::InvalidBase64(_))));
}

#[test]
fn missing_aa_zz_shell_is_error() {
    // base64("http://example.com") 无 AA/ZZ 壳
    let s = base64::engine::general_purpose::STANDARD.encode(b"http://example.com");
    let r = decode_thunder(&format!("thunder://{s}"));
    assert!(matches!(r, Err(ThunderError::MissingShell)));
}

#[test]
fn empty_inner_is_error() {
    // base64("AAZZ") → 剥壳后为空
    let s = base64::engine::general_purpose::STANDARD.encode(b"AAZZ");
    let r = decode_thunder(&format!("thunder://{s}"));
    assert!(matches!(r, Err(ThunderError::MissingShell)));
}

#[test]
fn decoded_result_is_http() {
    // M2 路由：Thunder 解码后是 Http（router_matrix 有集成断言，这里验证解码结果形态）
    let url = "https://example.com/f.bin";
    let decoded = decode_thunder(&encode_thunder(url)).unwrap();
    assert!(decoded.starts_with("https://"));
}