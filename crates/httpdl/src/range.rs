//! Range 探测（§14）：GET Range: bytes=0-0 判定服务器是否支持 Range，并取 ETag/总长。
//! 206 → 支持（多连接/续传能力）；200 → 忽略 Range（单连接流式）；416 → 范围非法（重下）。

use smart_dl_core::types::EngineError;
use std::time::Duration;

/// 探测结果（M4a：M4b 多连接/续传的输入）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    /// 服务器尊重 Range（探测响应 206）。
    pub range_supported: bool,
    pub etag: Option<String>,
    /// 文件总长（Content-Range 或 Content-Length）。
    pub total: Option<u64>,
}

/// 探测单个 URL。`headers` 为任务级自定义头（如 Referer/Cookie）。
pub async fn probe_range(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<Probe, EngineError> {
    let mut req = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .timeout(Duration::from_secs(30));
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| EngineError::Other(e.to_string()))?;
    let status = resp.status();
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    match status {
        reqwest::StatusCode::PARTIAL_CONTENT => {
            let total = content_range_total(resp.headers())
                .or(resp.content_length());
            Ok(Probe {
                range_supported: true,
                etag,
                total,
            })
        }
        reqwest::StatusCode::OK => Ok(Probe {
            range_supported: false,
            etag,
            total: resp.content_length(),
        }),
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE => Ok(Probe {
            range_supported: false,
            etag,
            // 416 通常带 Content-Range: bytes */TOTAL → 仍可取总长
            total: content_range_total(resp.headers()),
        }),
        other => Err(EngineError::Other(format!("probe status {other}"))),
    }
}

/// 解析 `Content-Range: bytes 0-0/TOTAL`（或 `bytes */TOTAL`）的总长。
fn content_range_total(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cr| cr.rsplit('/').next())
        .and_then(|t| t.parse().ok())
}