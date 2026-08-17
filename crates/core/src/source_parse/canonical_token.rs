//! HTTP URL 归一化与 token 参数黑名单（§7 + D34）。
//! 命中黑名单的 query 从 identity 剔除；其余 query 参与 identity。

/// 精确 token 参数名（小写比较）。
pub const TOKEN_PARAM_BLACKLIST: &[&str] = &["token", "sig", "signature", "expires", "auth"];

/// 前缀类签名参数（小写比较）。
const TOKEN_PARAM_PREFIXES: &[&str] = &["x-amz-", "x-goog-", "x-tencent-", "x-qiniu-"];

fn is_token_param(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if TOKEN_PARAM_BLACKLIST.contains(&lower.as_str()) {
        return true;
    }
    TOKEN_PARAM_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// 归一化 HTTP URL → (identity, token_sensitive)。
/// 去 fragment；剔除 token/signature 类 query（D34）；其余 query 保序参与 identity。
pub fn normalize_http_url(url: &str) -> (String, bool) {
    let no_frag = url.split('#').next().unwrap_or(url);
    let (base, query) = match no_frag.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (no_frag, None),
    };
    let Some(q) = query else {
        return (base.to_string(), false);
    };
    let mut kept: Vec<&str> = Vec::new();
    let mut sensitive = false;
    for pair in q.split('&') {
        let name = pair.split('=').next().unwrap_or("");
        if is_token_param(name) {
            sensitive = true;
        } else {
            kept.push(pair);
        }
    }
    let identity = if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    };
    (identity, sensitive)
}