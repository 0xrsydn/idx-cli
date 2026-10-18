//! Bandarmology over a broker summary: is buying more concentrated in a few
//! brokers than selling? No network; see docs/ARCHITECTURE.md.
//!
//! The summary lists the top 10 buyers and the top 10 sellers by gross lots.
//! For N in 1, 3, 5 and 10 the top N buyers' share of the value bought is set
//! against the top N sellers' share of the value sold. For all investors both
//! are the value traded, so this is the top N's buy value less their sell
//! value as a share of it; for one investor side (foreign or domestic), each
//! is that side's own buying or selling.
//!
//! | top-N buy share - sell share, %    | signal             |
//! |------------------------------------|--------------------|
//! | >= +15                             | `big_accumulation` |
//! | +5 to +15                          | `accumulation`     |
//! | within +-5                         | `neutral`          |
//! | -15 to -5                          | `distribution`     |
//! | <= -15                             | `big_distribution` |
//!
//! The thresholds follow fzn0x/idx (MIT), which set them against the 40 most
//! traded IDX stocks in October 2026. They are heuristics: a broker code is
//! not an investor, and gross figures overstate a broker that both bought
//! and sold (listed in `both_sides`).

use serde::{Deserialize, Serialize};

use crate::broker::types::{BrokerBoard, BrokerFlow, BrokerInvestor, BrokerSummary};
use crate::error::IdxError;
use chrono::NaiveDate;

/// Names the rules below; bumped when they change.
pub const METHOD: &str = "top_concentration_v1";
/// Tiers every report carries.
pub const TIERS: [u8; 4] = [1, 3, 5, 10];
/// Tier behind the headline signal unless asked otherwise.
pub const DEFAULT_TOP: u8 = 5;
/// Imbalance, in basis points of value traded, from which a tier reads
/// accumulation (or, negative, distribution).
pub const ACCUMULATION_BPS: i64 = 500;
/// Imbalance from which the reading is big.
pub const BIG_BPS: i64 = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BandarSignal {
    BigAccumulation,
    Accumulation,
    Neutral,
    Distribution,
    BigDistribution,
}

impl BandarSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BigAccumulation => "big_accumulation",
            Self::Accumulation => "accumulation",
            Self::Neutral => "neutral",
            Self::Distribution => "distribution",
            Self::BigDistribution => "big_distribution",
        }
    }

    pub fn from_imbalance_bps(bps: i64) -> Self {
        match bps {
            b if b >= BIG_BPS => Self::BigAccumulation,
            b if b >= ACCUMULATION_BPS => Self::Accumulation,
            b if b <= -BIG_BPS => Self::BigDistribution,
            b if b <= -ACCUMULATION_BPS => Self::Distribution,
            _ => Self::Neutral,
        }
    }
}

/// The top N buyers against the top N sellers. Every figure is `None` when
/// a broker in the tier lacks it, or when nothing traded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandarTier {
    pub top: u8,
    /// Value the top N buyers bought, whole IDR.
    pub buy_value: Option<i64>,
    /// Value the top N sellers sold, whole IDR.
    pub sell_value: Option<i64>,
    /// `buy_value - sell_value`, whole IDR.
    pub imbalance_value: Option<i64>,
    /// `buy_share_bps - sell_share_bps`, computed exactly: for all investors,
    /// `imbalance_value` in basis points of the value traded.
    pub imbalance_bps: Option<i64>,
    /// The top N buyers' share of the value bought, basis points.
    pub buy_share_bps: Option<i64>,
    /// The top N sellers' share of the value sold, basis points.
    pub sell_share_bps: Option<i64>,
    /// Volume-weighted average price the top N buyers paid, whole IDR.
    pub buy_average: Option<i64>,
    /// Volume-weighted average price the top N sellers got, whole IDR.
    pub sell_average: Option<i64>,
    pub signal: Option<BandarSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandarReport {
    pub symbol: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub investor: BrokerInvestor,
    pub board: BrokerBoard,
    /// The rules applied, e.g. `top_concentration_v1`.
    pub method: String,
    /// Tier behind the headline `signal`.
    pub top: u8,
    /// The headline tier's signal; `None` when nothing traded or a figure
    /// is missing.
    pub signal: Option<BandarSignal>,
    /// The headline tier's imbalance, basis points of value traded.
    pub imbalance_bps: Option<i64>,
    pub tiers: Vec<BandarTier>,
    /// Value traded over the range, whole IDR; `None` for one investor side.
    pub total_value: Option<i64>,
    /// Value the selected investors bought, whole IDR.
    pub buy_value: Option<i64>,
    /// Value the selected investors sold, whole IDR.
    pub sell_value: Option<i64>,
    /// Average traded price, whole IDR.
    pub average_price: Option<i64>,
    /// Foreign net value, whole IDR; `None` for a filtered summary.
    pub foreign_net_value: Option<i64>,
    /// `foreign_net_value` in basis points of the value traded.
    pub foreign_net_bps: Option<i64>,
    /// Brokers among the top 10 on both sides, whose gross figures overstate
    /// which way they traded.
    pub both_sides: Vec<String>,
    /// The source rounded figures of a million and more to one decimal.
    pub rounded: bool,
}

/// The headline tier must be one the report carries.
pub fn check_top(top: u8) -> Result<u8, IdxError> {
    if TIERS.contains(&top) {
        Ok(top)
    } else {
        Err(IdxError::InvalidInput(format!(
            "top must be 1, 3, 5 or 10 (got {top})"
        )))
    }
}

/// `part` in basis points of `whole`, truncated toward zero so a negative
/// reading never reaches a threshold its magnitude has not; `None` without
/// a positive whole.
fn bps(part: i64, whole: Option<i64>) -> Option<i64> {
    let whole = whole.filter(|whole| *whole > 0)?;
    i64::try_from(i128::from(part) * 10_000 / i128::from(whole)).ok()
}

/// Sum of `field` over `flows`; `None` when any flow lacks it.
fn sum(flows: &[BrokerFlow], field: fn(&BrokerFlow) -> Option<i64>) -> Option<i64> {
    flows
        .iter()
        .try_fold(0_i64, |total, flow| total.checked_add(field(flow)?))
}

/// Value over shares, the shares taken from each broker's value and its
/// exact published average (lots of a million and more are rounded,
/// averages never are).
fn vwap(flows: &[BrokerFlow]) -> Option<i64> {
    let (mut value, mut shares) = (0.0_f64, 0.0_f64);
    for flow in flows {
        let (flow_value, average) = (flow.value?, flow.average_price?);
        if average <= 0 {
            return None;
        }
        value += flow_value as f64;
        shares += flow_value as f64 / average as f64;
    }
    (shares > 0.0).then(|| (value / shares).round() as i64)
}

pub fn bandar_tier(summary: &BrokerSummary, top: u8) -> BandarTier {
    let n = usize::from(top);
    let buyers = &summary.buyers[..n.min(summary.buyers.len())];
    let sellers = &summary.sellers[..n.min(summary.sellers.len())];
    let buy_value = sum(buyers, |flow| flow.value);
    let sell_value = sum(sellers, |flow| flow.value);
    let imbalance_value = buy_value
        .zip(sell_value)
        .and_then(|(buy, sell)| buy.checked_sub(sell));
    let imbalance_bps = match (buy_value, sell_value, summary.buy_value, summary.sell_value) {
        (Some(buy), Some(sell), Some(bought), Some(sold)) if bought > 0 && sold > 0 => {
            // buy/bought - sell/sold over one denominator, so equal totals
            // give exactly (buy - sell) / total.
            let (buy, sell, bought, sold) = (
                i128::from(buy),
                i128::from(sell),
                i128::from(bought),
                i128::from(sold),
            );
            i64::try_from((buy * sold - sell * bought) * 10_000 / (bought * sold)).ok()
        }
        _ => None,
    };
    BandarTier {
        top,
        buy_value,
        sell_value,
        imbalance_value,
        imbalance_bps,
        buy_share_bps: buy_value.and_then(|value| bps(value, summary.buy_value)),
        sell_share_bps: sell_value.and_then(|value| bps(value, summary.sell_value)),
        buy_average: vwap(buyers),
        sell_average: vwap(sellers),
        signal: imbalance_bps.map(BandarSignal::from_imbalance_bps),
    }
}

/// The full reading of one summary, headlined by the top `top` tier (one of
/// [`TIERS`]; see [`check_top`]).
pub fn analyze_bandar(summary: &BrokerSummary, top: u8) -> BandarReport {
    let tiers: Vec<BandarTier> = TIERS.iter().map(|n| bandar_tier(summary, *n)).collect();
    let headline = tiers
        .iter()
        .find(|tier| tier.top == top)
        .cloned()
        .unwrap_or_else(|| bandar_tier(summary, top));
    let mut both_sides: Vec<String> = summary
        .buyers
        .iter()
        .filter(|buyer| {
            summary
                .sellers
                .iter()
                .any(|seller| seller.code == buyer.code)
        })
        .map(|buyer| buyer.code.clone())
        .collect();
    both_sides.sort_unstable();
    BandarReport {
        symbol: summary.symbol.clone(),
        from: summary.from,
        to: summary.to,
        investor: summary.investor,
        board: summary.board,
        method: METHOD.to_string(),
        top,
        signal: headline.signal,
        imbalance_bps: headline.imbalance_bps,
        tiers,
        total_value: summary.total_value,
        buy_value: summary.buy_value,
        sell_value: summary.sell_value,
        average_price: summary.average_price,
        foreign_net_value: summary.foreign_net_value,
        foreign_net_bps: summary
            .foreign_net_value
            .and_then(|value| bps(value, summary.total_value)),
        both_sides,
        rounded: summary.rounded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(code: &str, value: Option<i64>, average: i64) -> BrokerFlow {
        BrokerFlow {
            rank: 0,
            code: code.to_string(),
            name: None,
            kind: None,
            lots: value.map(|value| value / average / 100),
            value,
            average_price: Some(average),
        }
    }

    fn summary(buyers: Vec<BrokerFlow>, sellers: Vec<BrokerFlow>, total: i64) -> BrokerSummary {
        let day = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        BrokerSummary {
            symbol: "TEST.JK".to_string(),
            from: day,
            to: day,
            investor: BrokerInvestor::All,
            board: BrokerBoard::All,
            buyers,
            sellers,
            total_value: Some(total),
            total_lots: None,
            average_price: None,
            buy_value: Some(total),
            sell_value: Some(total),
            foreign_net_value: Some(-total / 10),
            rounded: false,
        }
    }

    #[test]
    fn test_bandar_signal_thresholds_are_inclusive() {
        use BandarSignal::*;
        for (bps, signal) in [
            (1500, BigAccumulation),
            (1499, Accumulation),
            (500, Accumulation),
            (499, Neutral),
            (-499, Neutral),
            (-500, Distribution),
            (-1499, Distribution),
            (-1500, BigDistribution),
        ] {
            assert_eq!(BandarSignal::from_imbalance_bps(bps), signal, "{bps}");
        }
    }

    #[test]
    fn test_analyze_bandar_weighs_top_buyers_against_top_sellers() {
        let s = summary(
            vec![
                flow("AA", Some(400), 100),
                flow("BB", Some(200), 100),
                flow("CC", Some(100), 50),
            ],
            vec![
                flow("DD", Some(300), 110),
                flow("AA", Some(100), 110),
                flow("EE", Some(50), 110),
            ],
            1000,
        );
        let report = analyze_bandar(&s, 3);
        let top1 = &report.tiers[0];
        assert_eq!((top1.top, top1.imbalance_value), (1, Some(100)));
        assert_eq!(top1.imbalance_bps, Some(1000));
        assert_eq!(top1.signal, Some(BandarSignal::Accumulation));

        let top3 = &report.tiers[1];
        assert_eq!((top3.buy_value, top3.sell_value), (Some(700), Some(450)));
        assert_eq!(
            (top3.buy_share_bps, top3.sell_share_bps),
            (Some(7000), Some(4500))
        );
        assert_eq!(top3.imbalance_bps, Some(2500));
        // 700 IDR over 4 + 2 + 2 shares.
        assert_eq!(top3.buy_average, Some(88));
        assert_eq!(top3.sell_average, Some(110));

        // Short lists: every broker on a side is listed, so the sums hold.
        assert_eq!(report.tiers[3].imbalance_value, Some(250));
        assert_eq!(
            (report.top, report.signal),
            (3, Some(BandarSignal::BigAccumulation))
        );
        assert_eq!(report.imbalance_bps, Some(2500));
        assert_eq!(report.foreign_net_bps, Some(-1000));
        assert_eq!(report.both_sides, vec!["AA".to_string()]);
        assert_eq!(report.method, METHOD);
    }

    #[test]
    fn test_bandar_tier_thresholds_are_symmetric_for_sign() {
        let reading = |buy: i64, sell: i64| {
            let s = summary(
                vec![flow("AA", Some(buy), 100)],
                vec![flow("BB", Some(sell), 100)],
                2_100_000_000,
            );
            let tier = bandar_tier(&s, 1);
            (tier.imbalance_bps, tier.signal)
        };
        // -4.9952% and -14.9952% stay short of the thresholds, like their
        // mirror images.
        assert_eq!(
            reading(504_300_000, 609_200_000),
            (Some(-499), Some(BandarSignal::Neutral))
        );
        assert_eq!(
            reading(609_200_000, 504_300_000),
            (Some(499), Some(BandarSignal::Neutral))
        );
        assert_eq!(
            reading(504_300_000, 819_200_000),
            (Some(-1499), Some(BandarSignal::Distribution))
        );
        assert_eq!(
            reading(819_200_000, 504_300_000),
            (Some(1499), Some(BandarSignal::Accumulation))
        );
    }

    #[test]
    fn test_bandar_tier_weighs_one_side_against_its_own_buying_and_selling() {
        // Foreign investors bought 1,000 and sold 4,000: the top buyer took
        // half of the buying, the top seller a quarter of the selling.
        let mut s = summary(
            vec![flow("AA", Some(500), 100)],
            vec![flow("BB", Some(1_000), 100)],
            0,
        );
        s.investor = BrokerInvestor::Foreign;
        s.total_value = None;
        s.buy_value = Some(1_000);
        s.sell_value = Some(4_000);
        s.foreign_net_value = Some(-3_000);
        let report = analyze_bandar(&s, 1);
        let tier = &report.tiers[0];
        assert_eq!(
            (tier.buy_share_bps, tier.sell_share_bps),
            (Some(5000), Some(2500))
        );
        assert_eq!(tier.imbalance_bps, Some(2500));
        assert_eq!(tier.imbalance_value, Some(-500));
        assert_eq!(report.signal, Some(BandarSignal::BigAccumulation));
        assert_eq!(report.foreign_net_bps, None, "no total to weigh it against");

        // Without the selling total (one side on one board) there is no reading.
        s.sell_value = None;
        assert_eq!(analyze_bandar(&s, 1).signal, None);
    }

    #[test]
    fn test_analyze_bandar_never_counts_a_missing_figure_as_zero() {
        let s = summary(
            vec![flow("AA", Some(400), 100), flow("BB", None, 100)],
            vec![flow("DD", Some(300), 100)],
            1000,
        );
        let report = analyze_bandar(&s, 3);
        assert_eq!(report.tiers[0].imbalance_bps, Some(1000));
        let top3 = &report.tiers[1];
        assert_eq!(
            (top3.buy_value, top3.imbalance_bps, top3.signal),
            (None, None, None)
        );
        assert_eq!(top3.buy_average, None);
        assert_eq!(report.signal, None);
    }

    #[test]
    fn test_analyze_bandar_without_trades_has_no_signal() {
        let report = analyze_bandar(&summary(vec![], vec![], 0), DEFAULT_TOP);
        assert!(report.tiers.iter().all(|tier| tier.signal.is_none()));
        assert_eq!((report.signal, report.imbalance_bps), (None, None));
        assert_eq!(report.tiers[2].buy_value, Some(0));
    }

    #[test]
    fn test_check_top_allows_only_report_tiers() {
        assert_eq!(check_top(5).unwrap(), 5);
        assert!(matches!(check_top(4), Err(IdxError::InvalidInput(_))));
    }
}
