//! 所有权边界模型（§9）：Acquisition、FallbackPolicy 与兜底决策。
//! 数据所有权归 Task；引擎只有传输权；恢复归 Rust；来源替换仅 Router/用户。

use serde::{Deserialize, Serialize};

/// 候选数据集的来源种类。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum AcqKind {
    Bt,
    Http,
    Ftp,
    Provider,
}

/// 单条候选数据（Acquisition 只是候选数据集，不是所有权，§9）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Acquisition {
    pub kind: AcqKind,
    pub engine_id: String,
    pub engine_task_id: String,
    pub state: AcqState,
    pub done: u64,
    pub total: u64,
    pub started_at_unix: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum AcqState {
    #[default]
    Pending,
    Active,
    Done,
    Failed,
}

/// 双源都只有半成品时的取舍（§9 D23）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum KeepLarger {
    #[default]
    KeepLarger,
}

/// 兜底策略（§9 D23 默认值冻结）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FallbackPolicy {
    pub bt_ratio_to_continue: f64,
    pub allow_parallel_disk: bool,
    pub on_both_partial: KeepLarger,
    pub max_provider_redownloads: u32,
}

impl Default for FallbackPolicy {
    fn default() -> Self {
        FallbackPolicy {
            bt_ratio_to_continue: 0.5,
            allow_parallel_disk: false,
            on_both_partial: KeepLarger::KeepLarger,
            max_provider_redownloads: 2,
        }
    }
}

/// 自动兜底决策（§9/§10 转换表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FallbackDecision {
    /// 可并行（allow_parallel_disk=true 且 BT 进度 < ratio）。
    Auto,
    /// 允许兜底，但需先暂停 BT（串行，禁双份占盘）。
    RequiresPauseFirst,
    /// 拒绝自动兜底（BT 进度 >= ratio，仅手动 fallback 命令）。
    ManualOnly,
}

/// 依据 BT 进度判定自动兜底（§9：BT <50% 允许；>=50% 仅手动）。
pub fn decide_auto_fallback(bt_progress: f64, policy: &FallbackPolicy) -> FallbackDecision {
    if bt_progress >= policy.bt_ratio_to_continue {
        return FallbackDecision::ManualOnly;
    }
    if policy.allow_parallel_disk {
        FallbackDecision::Auto
    } else {
        FallbackDecision::RequiresPauseFirst
    }
}

/// metadata 超时动作（Q-B9 写死：绝不自动触发 Provider，仅置 FallbackAvailable 标志）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetadataAction {
    KeepBt { fallback_available: bool },
}

pub fn on_metadata_timeout() -> MetadataAction {
    MetadataAction::KeepBt {
        fallback_available: true,
    }
}
