//! M6: WS 背压（D36）——队列上限 256、满丢最旧非关键事件、monotonic seq、
//! 客户端跳号 → snapshot_upto 补拉；掉队客户端重连 → drain 快照重同步。

use smart_dl_daemon::events::SchedulerEvent;
use smart_dl_daemon::ws::WsHub;

fn progress(task: &str, done: u64) -> SchedulerEvent {
    SchedulerEvent::Progress {
        task_id: task.into(),
        done,
        total: 1000,
    }
}

#[test]
fn seq_is_monotonic_across_publishes() {
    let hub = WsHub::new();
    for i in 1..=10 {
        hub.publish(progress("t", i));
    }
    assert_eq!(hub.last_seq(), 10);
    let seqs: Vec<u64> = hub.drain().iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=10).collect::<Vec<_>>());
}

#[test]
fn full_queue_drops_oldest_non_critical() {
    // 容量 3：填 3 个后新事件进 → 丢最旧非关键（Progress），关键事件保留
    let hub = WsHub::with_capacity(3);
    hub.publish(SchedulerEvent::TaskCreated {
        task_id: "t".into(),
    }); // 关键? TaskCreated 非关键
    hub.publish(progress("t", 1)); // 非关键
    hub.publish(SchedulerEvent::Completed {
        task_id: "t".into(),
    }); // 关键
    hub.publish(progress("t", 2)); // 满 → 丢最早非关键（Progress#2? 顺序上最先是的 TaskCreated）

    let drained = hub.drain();
    let remaining: Vec<&SchedulerEvent> = drained.iter().map(|e| &e.event).collect();
    // 队列 3：保留 TaskCreated(最早非关键? 丢的是 Progress#1?) ——
    // 规则：满时从最旧开始丢第一个非关键事件。队列 [TaskCreated, Progress1, Completed]
    // 最旧非关键 = TaskCreated → 丢 → [Progress1, Completed, Progress2]
    assert_eq!(remaining.len(), 3);
    assert!(
        remaining.iter().any(|&e| *e
            == SchedulerEvent::Completed {
                task_id: "t".into()
            }),
        "关键事件 Completed 不得被丢"
    );
}

#[test]
fn all_critical_queue_drops_oldest_anyway() {
    // 队里全是关键事件 → 极端仍丢队头（防死锁）
    let hub = WsHub::with_capacity(2);
    hub.publish(SchedulerEvent::Completed {
        task_id: "a".into(),
    });
    hub.publish(SchedulerEvent::Failed {
        task_id: "b".into(),
        reason: "x".into(),
    });
    hub.publish(SchedulerEvent::Completed {
        task_id: "c".into(),
    });
    let seqs: Vec<u64> = hub.drain().iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![2, 3], "队头 seq1 被丢，保留最新");
}

#[test]
fn snapshot_upto_fills_gap_after_detect_jump() {
    let hub = WsHub::new();
    hub.publish(SchedulerEvent::TaskCreated {
        task_id: "t".into(),
    }); // seq1
    hub.publish(progress("t", 1)); // seq2
                                   // 客户端拿到 seq1 后掉线重连，期间 seq2..=seq4 发生
    hub.publish(progress("t", 2)); // seq3
    hub.publish(SchedulerEvent::Completed {
        task_id: "t".into(),
    }); // seq4
        // 客户端已消费 seq1，发现跳号 → snapshot_upto(1) 补齐 2..=4
    let gap: Vec<u64> = hub.snapshot_upto(1).iter().map(|e| e.seq).collect();
    assert_eq!(gap, vec![2, 3, 4]);
    assert_eq!(hub.snapshot_upto(4).len(), 0, "无跳号 → 无补拉");
}

#[test]
fn drain_resyncs_a_lagged_client() {
    let hub = WsHub::new();
    hub.publish(SchedulerEvent::TaskCreated {
        task_id: "a".into(),
    });
    hub.publish(SchedulerEvent::TaskCreated {
        task_id: "b".into(),
    });
    // 掉队客户端重连 → drain 全量重同步（含历史 seq）
    let all: Vec<u64> = hub.drain().iter().map(|e| e.seq).collect();
    assert_eq!(all, vec![1, 2]);
    let again: Vec<u64> = hub.drain().iter().map(|e| e.seq).collect();
    assert!(again.is_empty(), "drain 幂等：消费后不重复");
}

#[test]
fn critical_events_are_not_droppable() {
    // 满队时只丢非关键：Progress 可丢；Completed/Failed/Error 不可丢
    let hub = WsHub::with_capacity(2);
    hub.publish(progress("t", 1));
    hub.publish(SchedulerEvent::Error {
        task_id: "t".into(),
        message: "e".into(),
    });
    hub.publish(SchedulerEvent::Failed {
        task_id: "t".into(),
        reason: "r".into(),
    });
    hub.publish(progress("t", 2));
    let drained = hub.drain();
    let events: Vec<&SchedulerEvent> = drained.iter().map(|e| &e.event).collect();
    assert!(events.iter().any(|&e| *e
        == SchedulerEvent::Error {
            task_id: "t".into(),
            message: "e".into()
        }));
    assert!(events.iter().any(|&e| *e
        == SchedulerEvent::Failed {
            task_id: "t".into(),
            reason: "r".into()
        }));
}
