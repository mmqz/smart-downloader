//! v1 启发式路由（§10.2 D18）：能力路由 + 热度阈值。

use crate::heat::heat_score;
use crate::ownership::FallbackPolicy;
use crate::registry::{EngineRegistry, RoutingError};
use crate::types::DownloadSource;

/// 路由决策。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteDecision {
    /// 交给指定引擎（engine id）。
    Engine(String),
    /// 冷门 → 直接兜底（受 FallbackPolicy）。
    FallbackProvider,
    /// 无可用来源（ed2k 等）。
    Failed(RoutingError),
}

/// 路由器：能力路由（registry.select）+ 热度判定（§10.2）。
pub struct Router {
    registry: EngineRegistry,
    policy: FallbackPolicy,
}

impl Router {
    pub fn new(registry: EngineRegistry, policy: FallbackPolicy) -> Self {
        Router { registry, policy }
    }

    /// route(source, avg_peers, avg_seeds)：
    /// - BT 类源（Magnet/TorrentFile）热度 <0.3 → FallbackProvider（受策略，但冷门无进度，
    ///   progress=0 < ratio 恒真 → 允许）；否则 → Engine(engine_id)
    /// - 非 BT 类 → Engine(engine_id)
    /// - ed2k/无引擎 → Failed
    pub fn route(&self, source: &DownloadSource, avg_peers: u32, avg_seeds: u32) -> RouteDecision {
        let engine_id = match self.registry.select(source) {
            Ok(id) => id,
            Err(e) => return RouteDecision::Failed(e),
        };
        let is_bt = matches!(
            source,
            DownloadSource::Magnet(_) | DownloadSource::TorrentFile(_)
        );
        if is_bt {
            let h = heat_score(avg_peers, avg_seeds);
            if h < 0.3 {
                // 冷门：只要策略允许（BT 进度 < ratio 恒真，冷门无进度）→ 直接兜底
                if self.policy.bt_ratio_to_continue > 0.0 {
                    return RouteDecision::FallbackProvider;
                }
            }
        }
        RouteDecision::Engine(engine_id)
    }
}
