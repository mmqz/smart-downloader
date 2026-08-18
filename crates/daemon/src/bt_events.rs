//! BT alert 事件流（feature `bt`）：后台轮询 btcore alert → 推进任务状态 + WS 事件广播。
//!
//! v1 范围：状态**主动推进**（State+Finished → Seeding，State+Error → Failed）+ 事件广播
//! （`Completed` / `Failed` / `StateChanged`）。进度/速度字段不在 alert 内，仍由快照
//! 实时拉取引擎状态提供——事件流只负责"被动拉取看不到的"终态推进与推送。
//!
//! 匹配键：`alert.ih` 对 `TaskRecord.engine_tid`（内核返回的原始大小写，比较时归一化）。

use std::sync::Arc;
use std::time::Duration;

use smart_dl_btcore::{Alert, AlertKind, BtCore, StateSubKind};
use smart_dl_core::state_machine::TaskState;
use smart_dl_core::types::EngineKind;

use crate::events::SchedulerEvent;
use crate::state::DaemonState;

/// alert → 状态迁移的纯函数（不触碰共享状态，单测友好）。
/// 返回 `(from, to)`；无迁移返回 `None`。
pub fn transition_for(now: &TaskState, a: &Alert) -> Option<(TaskState, TaskState)> {
    match (&a.kind, a.state_subkind()) {
        (AlertKind::State, StateSubKind::Finished)
            if matches!(
                now,
                TaskState::Downloading(EngineKind::Bt) | TaskState::Queued
            ) =>
        {
            Some((now.clone(), TaskState::Seeding))
        }
        (AlertKind::State, StateSubKind::Error) if *now != TaskState::Failed => {
            Some((now.clone(), TaskState::Failed))
        }
        _ => None,
    }
}

/// 启动 BT alert 消费循环：每 `interval` 弹一批（≤128）→ 应用迁移 → 广播事件。
/// 会话 pop 失败（内核不可用）静默跳过；任务已移除/无迁移的 alert 丢弃。
pub fn spawn_alert_loop(
    state: Arc<DaemonState>,
    core: Arc<BtCore>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let alerts = match core.pop_alerts(128) {
                Ok(v) => v,
                Err(_) => continue, // 会话暂时不可用：下轮再试
            };
            for alert in alerts {
                let Some(effect) = state.apply_bt_alert(&alert) else {
                    continue;
                };
                let hub = state.hub();
                hub.publish(SchedulerEvent::StateChanged {
                    task_id: effect.task_id.clone(),
                    from: effect.from,
                    to: effect.to.clone(),
                });
                match &effect.to {
                    TaskState::Seeding => {
                        hub.publish(SchedulerEvent::Completed {
                            task_id: effect.task_id.clone(),
                        });
                    }
                    TaskState::Failed => {
                        hub.publish(SchedulerEvent::Failed {
                            task_id: effect.task_id.clone(),
                            reason: effect.message.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }
    })
}
