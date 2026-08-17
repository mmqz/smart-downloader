//! M2: 热度评分公式边界（§10.2，D18）。
//! 热度 = clamp(avg_peers/50,0,1)*0.7 + clamp(avg_seeds/10,0,1)*0.3

use smart_dl_core::heat::{heat_level, heat_score, HeatEvaluator, HeatLevel};

#[test]
fn zero_peers_zero_seeds_is_cold() {
    assert!(heat_score(0, 0) < 0.3);
}

#[test]
fn fifty_peers_ten_seeds_is_hot() {
    assert!(heat_score(50, 10) >= 0.7);
}

#[test]
fn middle_is_middle() {
    // score(20,3) = 0.4*0.7 + 0.3*0.3 = 0.37
    let s = heat_score(20, 3);
    assert!((0.3..0.7).contains(&s), "score={s}");
}

#[test]
fn saturation_bounds() {
    // 超出上限被 clamp
    assert_eq!(heat_score(500, 100), 1.0);
    assert_eq!(heat_score(0, 0), 0.0);
}

#[test]
fn level_classification() {
    assert_eq!(heat_level(heat_score(50, 10)), HeatLevel::Hot);
    assert_eq!(heat_level(heat_score(20, 3)), HeatLevel::Warm);
    assert_eq!(heat_level(heat_score(0, 0)), HeatLevel::Cold);
    // 边界：0.7 恰好是 Hot；0.3 恰好进入 Warm（路由判定 >=0.3 → BT 可行）
    assert_eq!(heat_level(0.7), HeatLevel::Hot);
    assert_eq!(heat_level(0.3), HeatLevel::Warm);
    assert_eq!(heat_level(0.29), HeatLevel::Cold);
}

#[test]
fn evaluator_wraps_formula() {
    let ev = HeatEvaluator;
    assert_eq!(ev.score(50, 10), heat_score(50, 10));
}
