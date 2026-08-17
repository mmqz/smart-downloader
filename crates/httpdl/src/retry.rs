//! 重试退避（§14 D25）：429/5xx 退避 1/2/4/8s×4；403 → 查认证；404 → 文件级失败。
//! 纯函数（可注入时钟的调用方决定何时 sleep，测试不真等）。

use std::time::Duration;

/// 指数退避。attempt 从 1 起：1s, 2s, 4s, 8s（封顶 max）。
#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    pub base: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            base: Duration::from_secs(1),
            max: Duration::from_secs(8),
        }
    }
}

impl Backoff {
    /// 第 `attempt` 次重试前的等待（attempt ≥ 1）。
    pub fn next_delay(&self, attempt: u32) -> Duration {
        let exp = 1u32
            .checked_shl(attempt.saturating_sub(1))
            .unwrap_or(u32::MAX);
        self.base.saturating_mul(exp).min(self.max)
    }
}

/// 该状态码是否值得重试（D25：408/429/5xx；403/404 是终态）。
pub fn should_retry(status: u16, attempt: u32, max_attempts: u32) -> bool {
    if attempt >= max_attempts {
        return false;
    }
    matches!(status, 408 | 429 | 500..=599)
}
