//! 多连接段下载器（M4b）：并行下载各段到 .part（seek 定位，段不相交无锁），
//! 段失败 → 镜像轮换（列表内依次尝试）。限速（RateLimiter）跨段共享。

use crate::rate::RateLimiter;
use crate::static_split::Segment;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

/// 并行下载所有段。`mirrors` 为候选源列表：某段在某源失败 → 下一源重试该段。
/// 全源失败 → Err（调用方决定重试/报错）。`limiter` 为跨段共享限速器（0 = 不限）。
pub async fn download_segments(
    client: &reqwest::Client,
    part: &Path,
    segments: &[Segment],
    mirrors: &[String],
    total: u64,
    limiter: Arc<RateLimiter>,
) -> Result<(), String> {
    // 预分配 .part（含续传场景：旧 .part 保留，只写缺失段；不截断）
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(part)
        .map_err(|e| e.to_string())?;
    f.set_len(total).map_err(|e| e.to_string())?;
    drop(f);

    let mut handles = Vec::new();
    for seg in segments {
        let client = client.clone();
        let part = part.to_path_buf();
        let mirrors = mirrors.to_vec();
        let seg = *seg;
        let limiter = limiter.clone();
        handles.push(tokio::spawn(async move {
            for m in &mirrors {
                match download_segment(&client, m, seg, &limiter).await {
                    Ok(bytes) => {
                        write_segment(&part, seg, &bytes).map_err(|e| e.to_string())?;
                        return Ok::<(), String>(());
                    }
                    Err(_) => continue, // 下一 mirror
                }
            }
            Err("all mirrors failed for segment".to_string())
        }));
    }
    for h in handles {
        h.await.map_err(|e| e.to_string())??;
    }
    Ok(())
}

/// 下载单个段（Range: bytes=start-end），流式读取 + 限速，校验长度。
async fn download_segment(
    client: &reqwest::Client,
    url: &str,
    seg: Segment,
    limiter: &RateLimiter,
) -> Result<Vec<u8>, String> {
    let mut resp = client
        .get(url)
        .header(
            reqwest::header::RANGE,
            format!("bytes={}-{}", seg.start, seg.end),
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("segment status {status}"));
    }
    let mut buf = Vec::with_capacity(seg.len() as usize);
    loop {
        let chunk = resp.chunk().await.map_err(|e| e.to_string())?;
        let Some(chunk) = chunk else { break };
        limiter.wait(chunk.len() as u64).await;
        buf.extend_from_slice(&chunk);
    }
    if buf.len() as u64 != seg.len() {
        return Err(format!(
            "segment length {} != expected {}",
            buf.len(),
            seg.len()
        ));
    }
    Ok(buf)
}

/// 段写入 .part：seek 到段起点（段不相交 → 并发写无锁冲突）。
fn write_segment(part: &Path, seg: Segment, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().write(true).open(part)?;
    f.seek(SeekFrom::Start(seg.start))?;
    f.write_all(bytes)?;
    Ok(())
}
