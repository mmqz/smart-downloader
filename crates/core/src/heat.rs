//! 生态热度 v1（§10.2 D18）：线性加权 + clamp 饱和。

/// 热度评分：clamp(avg_peers/50,0,1)*0.7 + clamp(avg_seeds/10,0,1)*0.3
pub fn heat_score(avg_peers: u32, avg_seeds: u32) -> f64 {
    let p = (avg_peers as f64 / 50.0).clamp(0.0, 1.0);
    let s = (avg_seeds as f64 / 10.0).clamp(0.0, 1.0);
    p * 0.7 + s * 0.3
}

/// 热度分级（与路由判定一致：>=0.7 Hot；0.3..0.7 Warm；<0.3 Cold）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeatLevel {
    Hot,
    Warm,
    Cold,
}

pub fn heat_level(score: f64) -> HeatLevel {
    if score >= 0.7 {
        HeatLevel::Hot
    } else if score >= 0.3 {
        HeatLevel::Warm
    } else {
        HeatLevel::Cold
    }
}

/// 热度评估器（M2 输出契约类型；无状态）。
pub struct HeatEvaluator;

impl HeatEvaluator {
    pub fn score(&self, avg_peers: u32, avg_seeds: u32) -> f64 {
        heat_score(avg_peers, avg_seeds)
    }
}
