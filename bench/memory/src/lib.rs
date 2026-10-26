//! メモリ予算の定義と判定(設計書 §4「メモリ予算」「計測と回帰防止」)。
//!
//! 判定は純粋関数で、計測(OS 依存)は `workload` に分離する。
//! M0 で実測できるのは「Rust 側」シナリオだけで、Chromium を伴う3シナリオはフェーズ1で追加する。

mod workload;

pub use workload::measure_rust_side_mb;

/// 前日比の増加がこの割合を超えたら回帰とみなす(5%)。
pub const REGRESSION_RATIO: f64 = 1.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// 起動直後(新規タブのみ): Browser + GPU + Crashpad。
    Startup,
    /// 一般的なページ1枚: 上記 + Renderer 1個。
    SinglePage,
    /// タブ20枚(閲覧中1枚)。
    Tabs20,
    /// Browser プロセス内の Rust ヒープ(core + サービス)。
    RustSide,
}

impl Scenario {
    pub const ALL: [Scenario; 4] = [
        Scenario::Startup,
        Scenario::SinglePage,
        Scenario::Tabs20,
        Scenario::RustSide,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scenario::Startup => "startup",
            Scenario::SinglePage => "single_page",
            Scenario::Tabs20 => "tabs_20",
            Scenario::RustSide => "rust_side",
        }
    }

    /// 予算(MB)。Private メモリのプロセス合計。
    /// Chromium を伴う 3 つは **CEF 基準の仮置き**(docs/roadmap-cef.md)。release ビルドの参考値
    /// (起動 219〜242 / ページ 1 枚 268〜289 / 20 タブ約 1,660)に約 15% の余裕を足して丸めた。
    /// 設計書の Content API 前提の値(150 / 250 / 600)は将来の目標として同じ文書に残す。
    /// 空きメモリが十分な状態で再計測したら置き換える。
    pub fn budget_mb(self) -> f64 {
        match self {
            Scenario::Startup => 280.0,
            Scenario::SinglePage => 330.0,
            Scenario::Tabs20 => 1900.0,
            Scenario::RustSide => 20.0,
        }
    }

    /// M0 で計測できるか(Chromium が必要なものは不可)。
    pub fn measurable_in_m0(self) -> bool {
        matches!(self, Scenario::RustSide)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Pass { mb: f64 },
    OverBudget { mb: f64, budget_mb: f64 },
    Regression { mb: f64, previous_mb: f64 },
}

impl Verdict {
    pub fn is_failure(&self) -> bool {
        !matches!(self, Verdict::Pass { .. })
    }
}

/// 中央値。空なら `None`。ノイズに強くするため複数回の計測値を集約する。
pub fn median(samples: &[f64]) -> Option<f64> {
    if samples.is_empty() || samples.iter().any(|v| v.is_nan()) {
        return None;
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let mid = v.len() / 2;
    Some(if v.len() % 2 == 1 {
        v[mid]
    } else {
        (v[mid - 1] + v[mid]) / 2.0
    })
}

/// 中央値が予算超過、または前回比 5% 超の増加なら失敗。予算超過を優先して報告する。
pub fn evaluate(scenario: Scenario, samples: &[f64], previous_mb: Option<f64>) -> Option<Verdict> {
    let mb = median(samples)?;
    let budget_mb = scenario.budget_mb();
    if mb > budget_mb {
        return Some(Verdict::OverBudget { mb, budget_mb });
    }
    match previous_mb {
        Some(prev) if prev > 0.0 && mb > prev * REGRESSION_RATIO => Some(Verdict::Regression {
            mb,
            previous_mb: prev,
        }),
        _ => Some(Verdict::Pass { mb }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_match_design_doc() {
        let b: Vec<f64> = Scenario::ALL.iter().map(|s| s.budget_mb()).collect();
        assert_eq!(b, [280.0, 330.0, 1900.0, 20.0]);
    }

    #[test]
    fn only_rust_side_is_measurable_in_m0() {
        let m: Vec<_> = Scenario::ALL
            .iter()
            .filter(|s| s.measurable_in_m0())
            .collect();
        assert_eq!(m, [&Scenario::RustSide]);
    }

    #[test]
    fn median_odd_even_and_degenerate() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[1.0, f64::NAN]), None);
    }

    #[test]
    fn within_budget_passes() {
        assert_eq!(
            evaluate(Scenario::RustSide, &[10.0], None),
            Some(Verdict::Pass { mb: 10.0 })
        );
        // ちょうど予算は合格。
        assert!(!evaluate(Scenario::RustSide, &[20.0], None)
            .unwrap()
            .is_failure());
    }

    #[test]
    fn over_budget_fails_and_uses_median_not_max() {
        let v = evaluate(Scenario::RustSide, &[21.0, 22.0, 23.0], None).unwrap();
        assert_eq!(
            v,
            Verdict::OverBudget {
                mb: 22.0,
                budget_mb: 20.0
            }
        );
        // 外れ値1回では落ちない。
        assert!(!evaluate(Scenario::RustSide, &[10.0, 11.0, 90.0], None)
            .unwrap()
            .is_failure());
    }

    #[test]
    fn five_percent_growth_is_a_regression_but_equal_is_not() {
        assert!(!evaluate(Scenario::Tabs20, &[105.0], Some(100.0))
            .unwrap()
            .is_failure());
        let v = evaluate(Scenario::Tabs20, &[105.1], Some(100.0)).unwrap();
        assert_eq!(
            v,
            Verdict::Regression {
                mb: 105.1,
                previous_mb: 100.0
            }
        );
    }

    #[test]
    fn over_budget_takes_priority_over_regression() {
        let v = evaluate(Scenario::RustSide, &[30.0], Some(10.0)).unwrap();
        assert!(matches!(v, Verdict::OverBudget { .. }));
    }

    #[test]
    fn shrinking_or_zero_previous_passes() {
        assert!(!evaluate(Scenario::Startup, &[90.0], Some(100.0))
            .unwrap()
            .is_failure());
        assert!(!evaluate(Scenario::Startup, &[90.0], Some(0.0))
            .unwrap()
            .is_failure());
    }

    #[test]
    fn no_samples_gives_no_verdict() {
        assert_eq!(evaluate(Scenario::Startup, &[], None), None);
    }
}
