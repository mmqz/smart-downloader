//! WsHub（§12 D36）：事件发布中枢——monotonic seq + 有界队列（默认 256）+ 背压
//! 丢最旧非关键；drain 全量重同步（掉队客户端重连）；snapshot_upto 跳号补拉。
//! 真 WS 升级端点在 http 层；本模块是纯逻辑，测试直连。

use crate::events::{Envelope, SchedulerEvent};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// 默认事件队列上限（D36：队列 256）。
pub const DEFAULT_CAPACITY: usize = 256;

pub struct WsHub {
    seq: AtomicU64,
    cap: usize,
    queue: Mutex<VecDeque<Envelope>>,
}

impl Default for WsHub {
    fn default() -> Self {
        WsHub::new()
    }
}

impl WsHub {
    pub fn new() -> Self {
        WsHub::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(cap: usize) -> Self {
        WsHub {
            seq: AtomicU64::new(0),
            cap,
            queue: Mutex::new(VecDeque::with_capacity(cap)),
        }
    }

    /// 发布事件：seq 严格递增；队列满 → 丢最旧非关键事件；队内全关键时
    /// 关键事件绝不移出（新非关键事件丢弃；新关键事件丢队头兜底防死锁）。
    pub fn publish(&self, event: SchedulerEvent) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let mut q = self.queue.lock().unwrap();
        if q.len() >= self.cap {
            match q.iter().position(|e| !e.event.is_critical()) {
                Some(i) => {
                    q.remove(i);
                    q.push_back(Envelope { seq, event });
                }
                None if event.is_critical() => {
                    q.pop_front();
                    q.push_back(Envelope { seq, event });
                }
                None => {
                    // 队内全关键 + 新事件非关键 → 丢新事件，关键全部保留
                }
            }
        } else {
            q.push_back(Envelope { seq, event });
        }
    }

    /// 消费全量（幂等：调用后队列清空）——掉队客户端重连重同步入口。
    pub fn drain(&self) -> Vec<Envelope> {
        let mut q = self.queue.lock().unwrap();
        q.drain(..).collect()
    }

    /// 补拉 seq > last_seen 的事件（客户端发现跳号 → GET /tasks/:id 前先补事件）。
    pub fn snapshot_upto(&self, last_seen: u64) -> Vec<Envelope> {
        self.queue
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.seq > last_seen)
            .cloned()
            .collect()
    }

    /// 已发布的最大 seq。
    pub fn last_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// 当前队列长度（背压观测）。
    pub fn len(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    /// 队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.queue.lock().unwrap().is_empty()
    }
}
