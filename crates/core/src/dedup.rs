//! 任务去重（§4 去重 + D34）：入队前查 canonical，重复 → DuplicateRejected。

use crate::identity::CanonicalId;
use crate::task::TaskId;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedupOutcome {
    Accepted,
    DuplicateRejected,
}

/// 去重索引（canonical_id → 任务）。
#[derive(Default)]
pub struct DedupIndex {
    map: HashMap<CanonicalId, TaskId>,
}

impl DedupIndex {
    pub fn new() -> Self {
        DedupIndex {
            map: HashMap::new(),
        }
    }

    /// 入队前检查。规则（D34）：
    /// - 键已存在且非"带 token 无 validator"→ DuplicateRejected
    /// - 带 token 无 validator：不自动认重（可能不同签名源），仍 Accepted
    pub fn check(&mut self, canonical: &CanonicalId, id: TaskId) -> DedupOutcome {
        if self.map.contains_key(canonical) {
            if canonical.token_sensitive && canonical.validator.is_none() {
                return DedupOutcome::Accepted;
            }
            return DedupOutcome::DuplicateRejected;
        }
        self.map.insert(canonical.clone(), id);
        DedupOutcome::Accepted
    }

    pub fn remove(&mut self, canonical: &CanonicalId) -> Option<TaskId> {
        self.map.remove(canonical)
    }
}