//! M6: 9 类 WS 事件（D36）serde 往返 + 字段对齐；Envelope（monotonic seq）。

use smart_dl_core::ownership::FallbackPolicy;
use smart_dl_core::state_machine::TaskState;
use smart_dl_core::types::{EngineKind, EngineState};
use smart_dl_daemon::events::{Envelope, SchedulerEvent};
use smart_dl_daemon::ws::WsHub;
use smart_dl_provider::ProviderRuntime;

fn roundtrip(e: SchedulerEvent) -> SchedulerEvent {
    let json = serde_json::to_string(&e).unwrap();
    serde_json::from_str(&json).unwrap()
}

#[test]
fn task_created_serde_roundtrip() {
    let e = SchedulerEvent::TaskCreated {
        task_id: "t1".into(),
    };
    assert_eq!(roundtrip(e.clone()), e);
    assert!(serde_json::to_string(&e)
        .unwrap()
        .contains("\"task_id\":\"t1\""));
}

#[test]
fn state_changed_serde_roundtrip() {
    let e = SchedulerEvent::StateChanged {
        task_id: "t2".into(),
        from: TaskState::Queued,
        to: TaskState::Downloading(EngineKind::Http),
    };
    assert_eq!(roundtrip(e.clone()), e);
}

#[test]
fn progress_and_speed_serde_roundtrip() {
    let p = SchedulerEvent::Progress {
        task_id: "t3".into(),
        done: 100,
        total: 1000,
    };
    let s = SchedulerEvent::Speed {
        task_id: "t3".into(),
        down_rate: 2048,
        up_rate: 512,
    };
    assert_eq!(roundtrip(p.clone()), p);
    assert_eq!(roundtrip(s.clone()), s);
}

#[test]
fn health_error_completed_failed_serde_roundtrip() {
    let cases = vec![
        SchedulerEvent::HealthEvent {
            task_id: "t4".into(),
            kind: smart_dl_daemon::health::HealthEventKind::LeechDetected,
        },
        SchedulerEvent::Error {
            task_id: "t5".into(),
            message: "disk full".into(),
        },
        SchedulerEvent::Completed {
            task_id: "t6".into(),
        },
        SchedulerEvent::Failed {
            task_id: "t7".into(),
            reason: "retries exceeded".into(),
        },
    ];
    for e in cases {
        assert_eq!(roundtrip(e.clone()), e);
    }
}

#[test]
fn duplicate_rejected_serde_roundtrip() {
    let e = SchedulerEvent::DuplicateRejected {
        task_id: "t8".into(),
        existing: "t1".into(),
    };
    assert_eq!(roundtrip(e.clone()), e);
}

#[test]
fn provider_status_event_serde_roundtrip() {
    // D36: 9 类事件 + ProviderStatus（运行态快照）
    let e = SchedulerEvent::ProviderStatus {
        provider: "mock".into(),
        runtime: ProviderRuntime::default(),
    };
    assert_eq!(roundtrip(e.clone()), e);
}

#[test]
fn envelope_carries_monotonic_seq() {
    let hub = WsHub::new();
    hub.publish(SchedulerEvent::TaskCreated {
        task_id: "a".into(),
    });
    hub.publish(SchedulerEvent::Completed {
        task_id: "a".into(),
    });
    let drained = hub.drain();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].seq, 1);
    assert_eq!(drained[1].seq, 2);
    assert_eq!(hub.last_seq(), 2);
    // Envelope serde（seq + 事件）
    let json = serde_json::to_string(&drained[0]).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    assert_eq!(back, drained[0]);
}

#[test]
fn state_change_flow_publishes_full_sequence() {
    // 状态机流：Queued→Downloading→Completed 对应事件序列
    let hub = WsHub::new();
    let flow = vec![
        SchedulerEvent::TaskCreated {
            task_id: "x".into(),
        },
        SchedulerEvent::StateChanged {
            task_id: "x".into(),
            from: TaskState::Queued,
            to: TaskState::Downloading(EngineKind::Http),
        },
        SchedulerEvent::Completed {
            task_id: "x".into(),
        },
    ];
    for e in flow {
        hub.publish(e);
    }
    let seqs: Vec<u64> = hub.drain().iter().map(|env| env.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3], "seq 必须单调递增无跳号");
}

// 让 EngineState/EngineKind/FallbackPolicy 出现在类型检查中（接口对齐守卫）
#[allow(dead_code)]
fn interface_guard() {
    let _ = EngineState::Downloading;
    let _ = FallbackPolicy::default();
}
