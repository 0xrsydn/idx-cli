use serde::{Deserialize, Serialize};

use crate::api::types::Fundamentals;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrowthReport {
    pub revenue_growth: Option<f64>,
    pub earnings_growth: Option<f64>,
    pub revenue_growth_pct: Option<f64>,
    pub earnings_growth_pct: Option<f64>,
    pub revenue_signal: String,
    pub earnings_signal: String,
    pub overall_signal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuationReport {
    pub pe_trailing: Option<f64>,
    pub pe_forward: Option<f64>,
    pub pb: Option<f64>,
    pub roe: Option<f64>,
    pub roe_pct: Option<f64>,
    pub net_margin: Option<f64>,
    pub net_margin_pct: Option<f64>,
    pub ev_ebitda: Option<f64>,
    pub pe_signal: String,
    pub pb_signal: String,
    pub roe_signal: String,
    pub margin_signal: String,
    pub ev_ebitda_signal: String,
    pub overall_signal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskReport {
    pub debt_to_equity: Option<f64>,
    pub current_ratio: Option<f64>,
    pub roa: Option<f64>,
    pub roa_pct: Option<f64>,
    pub de_signal: String,
    pub current_ratio_signal: String,
    pub overall_signal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundamentalReport {
    pub symbol: String,
    pub growth: GrowthReport,
    pub valuation: ValuationReport,
    pub risk: RiskReport,
    pub overall_signal: String,
}

pub fn analyze_growth(fundamentals: &Fundamentals) -> GrowthReport {
    let revenue_signal = growth_signal(fundamentals.revenue_growth);
    let earnings_signal = growth_signal(fundamentals.earnings_growth);
    let positive = ["strong", "moderate"];
    let negative = ["declining", "contracting"];

    let overall_signal = if revenue_signal == "no data" && earnings_signal == "no data" {
        "no data"
    } else if positive.contains(&revenue_signal) && positive.contains(&earnings_signal) {
        "growing"
    } else if negative.contains(&revenue_signal) && negative.contains(&earnings_signal) {
        "shrinking"
    } else if revenue_signal == "no data" || earnings_signal == "no data" {
        let other = if earnings_signal == "no data" {
            revenue_signal
        } else {
            earnings_signal
        };
        if negative.contains(&other) {
            "mixed"
        } else {
            "incomplete data"
        }
    } else {
        "mixed"
    };

    GrowthReport {
        revenue_growth: fundamentals.revenue_growth,
        earnings_growth: fundamentals.earnings_growth,
        revenue_growth_pct: ratio_pct(fundamentals.revenue_growth),
        earnings_growth_pct: ratio_pct(fundamentals.earnings_growth),
        revenue_signal: revenue_signal.to_string(),
        earnings_signal: earnings_signal.to_string(),
        overall_signal: overall_signal.to_string(),
    }
}

pub fn analyze_valuation(fundamentals: &Fundamentals) -> ValuationReport {
    let ev_ebitda = match (fundamentals.enterprise_value, fundamentals.ebitda) {
        (Some(ev), Some(ebitda)) if ebitda > 0 => Some(round2(ev as f64 / ebitda as f64)),
        _ => None,
    };

    let pe_signal = pe_signal(fundamentals.trailing_pe);
    let pb_signal = pb_signal(fundamentals.price_to_book);
    let ev_ebitda_signal = ev_ebitda_signal(ev_ebitda);
    let cheap = ["deep value", "undervalued"];
    let rich = ["premium", "expensive"];
    let price_signals: Vec<&str> = [pe_signal, pb_signal, ev_ebitda_signal]
        .into_iter()
        .filter(|signal| *signal != "no data")
        .collect();

    let overall_signal = if price_signals.is_empty() {
        "no data"
    } else {
        let cheap_count = price_signals
            .iter()
            .filter(|signal| cheap.contains(signal))
            .count();
        let rich_count = price_signals
            .iter()
            .filter(|signal| rich.contains(signal))
            .count();

        if cheap_count > price_signals.len() / 2 {
            "undervalued"
        } else if rich_count > price_signals.len() / 2 {
            "expensive"
        } else {
            "fairly valued"
        }
    };

    ValuationReport {
        pe_trailing: fundamentals.trailing_pe,
        pe_forward: fundamentals.forward_pe,
        pb: fundamentals.price_to_book,
        roe: fundamentals.return_on_equity,
        roe_pct: ratio_pct(fundamentals.return_on_equity),
        net_margin: fundamentals.profit_margins,
        net_margin_pct: ratio_pct(fundamentals.profit_margins),
        ev_ebitda,
        pe_signal: pe_signal.to_string(),
        pb_signal: pb_signal.to_string(),
        roe_signal: roe_signal(fundamentals.return_on_equity).to_string(),
        margin_signal: margin_signal(fundamentals.profit_margins).to_string(),
        ev_ebitda_signal: ev_ebitda_signal.to_string(),
        overall_signal: overall_signal.to_string(),
    }
}

pub fn analyze_risk(fundamentals: &Fundamentals) -> RiskReport {
    let de_signal = de_signal(fundamentals.debt_to_equity);
    let current_ratio_signal = cr_signal(fundamentals.current_ratio);

    let overall_signal = if de_signal == "no data" && current_ratio_signal == "no data" {
        "no data"
    } else if de_signal == "no data" || current_ratio_signal == "no data" {
        "incomplete data"
    } else if de_signal == "highly leveraged" || current_ratio_signal == "weak" {
        "high risk"
    } else if de_signal == "conservative" && matches!(current_ratio_signal, "strong" | "adequate") {
        "low risk"
    } else {
        "moderate risk"
    };

    RiskReport {
        debt_to_equity: fundamentals.debt_to_equity,
        current_ratio: fundamentals.current_ratio,
        roa: fundamentals.return_on_assets,
        roa_pct: ratio_pct(fundamentals.return_on_assets),
        de_signal: de_signal.to_string(),
        current_ratio_signal: current_ratio_signal.to_string(),
        overall_signal: overall_signal.to_string(),
    }
}

pub fn analyze_fundamental(symbol: &str, fundamentals: &Fundamentals) -> FundamentalReport {
    let growth = analyze_growth(fundamentals);
    let valuation = analyze_valuation(fundamentals);
    let risk = analyze_risk(fundamentals);

    let overall_signal = if growth.overall_signal == "growing"
        && valuation.overall_signal != "expensive"
        && risk.overall_signal != "high risk"
    {
        "healthy"
    } else if growth.overall_signal == "shrinking" && risk.overall_signal == "high risk" {
        "weak"
    } else if [
        &growth.overall_signal,
        &valuation.overall_signal,
        &risk.overall_signal,
    ]
    .into_iter()
    .all(|signal| *signal == "no data")
    {
        "no data"
    } else {
        "mixed"
    };

    FundamentalReport {
        symbol: symbol.to_string(),
        growth,
        valuation,
        risk,
        overall_signal: overall_signal.to_string(),
    }
}

fn growth_signal(value: Option<f64>) -> &'static str {
    let Some(value) = value else {
        return "no data";
    };
    if value >= 0.20 {
        "strong"
    } else if value >= 0.10 {
        "moderate"
    } else if value >= 0.0 {
        "slow"
    } else if value >= -0.10 {
        "declining"
    } else {
        "contracting"
    }
}

fn pe_signal(value: Option<f64>) -> &'static str {
    let Some(value) = value else {
        return "no data";
    };
    if value <= 0.0 {
        "no data"
    } else if value < 8.0 {
        "deep value"
    } else if value < 15.0 {
        "undervalued"
    } else if value < 25.0 {
        "fairly valued"
    } else if value < 40.0 {
        "premium"
    } else {
        "expensive"
    }
}

fn pb_signal(value: Option<f64>) -> &'static str {
    let Some(value) = value else {
        return "no data";
    };
    if value <= 0.0 {
        "no data"
    } else if value < 1.0 {
        "deep value"
    } else if value < 2.0 {
        "undervalued"
    } else if value < 4.0 {
        "fairly valued"
    } else {
        "expensive"
    }
}

fn roe_signal(value: Option<f64>) -> &'static str {
    let Some(value) = value else {
        return "no data";
    };
    if value >= 0.20 {
        "excellent"
    } else if value >= 0.15 {
        "strong"
    } else if value >= 0.10 {
        "adequate"
    } else if value >= 0.0 {
        "weak"
    } else {
        "negative"
    }
}

fn margin_signal(value: Option<f64>) -> &'static str {
    let Some(value) = value else {
        return "no data";
    };
    if value >= 0.20 {
        "excellent"
    } else if value >= 0.10 {
        "healthy"
    } else if value >= 0.05 {
        "adequate"
    } else if value >= 0.0 {
        "thin"
    } else {
        "negative"
    }
}

fn ev_ebitda_signal(value: Option<f64>) -> &'static str {
    let Some(value) = value else {
        return "no data";
    };
    if value <= 0.0 {
        "no data"
    } else if value < 8.0 {
        "undervalued"
    } else if value < 14.0 {
        "fairly valued"
    } else if value < 20.0 {
        "premium"
    } else {
        "expensive"
    }
}

fn de_signal(value: Option<f64>) -> &'static str {
    let Some(value) = value else {
        return "no data";
    };
    if value < 0.0 {
        "negative equity"
    } else if value < 50.0 {
        "conservative"
    } else if value < 100.0 {
        "moderate"
    } else if value < 200.0 {
        "leveraged"
    } else {
        "highly leveraged"
    }
}

fn cr_signal(value: Option<f64>) -> &'static str {
    let Some(value) = value else {
        return "no data";
    };
    if value >= 2.0 {
        "strong"
    } else if value >= 1.5 {
        "adequate"
    } else if value >= 1.0 {
        "tight"
    } else {
        "weak"
    }
}

fn ratio_pct(value: Option<f64>) -> Option<f64> {
    value.map(|value| round2(value * 100.0))
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::{
        Fundamentals, analyze_fundamental, analyze_growth, analyze_risk, analyze_valuation,
        cr_signal, de_signal, ev_ebitda_signal, growth_signal, margin_signal, pb_signal, pe_signal,
        roe_signal,
    };

    fn sample_fundamentals() -> Fundamentals {
        Fundamentals {
            trailing_pe: Some(12.5),
            forward_pe: Some(11.0),
            price_to_book: Some(1.8),
            return_on_equity: Some(0.18),
            profit_margins: Some(0.12),
            return_on_assets: Some(0.06),
            revenue_growth: Some(0.12),
            earnings_growth: Some(0.22),
            debt_to_equity: Some(40.0),
            current_ratio: Some(1.6),
            enterprise_value: Some(120),
            ebitda: Some(15),
            market_cap: Some(100),
        }
    }

    #[test]
    fn growth_signal_thresholds_match_python() {
        assert_eq!(growth_signal(None), "no data");
        assert_eq!(growth_signal(Some(0.20)), "strong");
        assert_eq!(growth_signal(Some(0.10)), "moderate");
        assert_eq!(growth_signal(Some(0.0)), "slow");
        assert_eq!(growth_signal(Some(-0.10)), "declining");
        assert_eq!(growth_signal(Some(-0.11)), "contracting");
    }

    #[test]
    fn valuation_signal_thresholds_match_python() {
        assert_eq!(pe_signal(None), "no data");
        assert_eq!(pe_signal(Some(7.9)), "deep value");
        assert_eq!(pe_signal(Some(14.9)), "undervalued");
        assert_eq!(pe_signal(Some(24.9)), "fairly valued");
        assert_eq!(pe_signal(Some(39.9)), "premium");
        assert_eq!(pe_signal(Some(40.0)), "expensive");

        assert_eq!(pb_signal(Some(0.9)), "deep value");
        assert_eq!(pb_signal(Some(1.9)), "undervalued");
        assert_eq!(pb_signal(Some(3.9)), "fairly valued");
        assert_eq!(pb_signal(Some(4.0)), "expensive");

        assert_eq!(roe_signal(Some(0.20)), "excellent");
        assert_eq!(roe_signal(Some(0.15)), "strong");
        assert_eq!(roe_signal(Some(0.10)), "adequate");
        assert_eq!(roe_signal(Some(0.0)), "weak");
        assert_eq!(roe_signal(Some(-0.01)), "negative");

        assert_eq!(margin_signal(Some(0.20)), "excellent");
        assert_eq!(margin_signal(Some(0.10)), "healthy");
        assert_eq!(margin_signal(Some(0.05)), "adequate");
        assert_eq!(margin_signal(Some(0.0)), "thin");
        assert_eq!(margin_signal(Some(-0.01)), "negative");

        assert_eq!(ev_ebitda_signal(Some(7.9)), "undervalued");
        assert_eq!(ev_ebitda_signal(Some(13.9)), "fairly valued");
        assert_eq!(ev_ebitda_signal(Some(19.9)), "premium");
        assert_eq!(ev_ebitda_signal(Some(20.0)), "expensive");
    }

    #[test]
    fn risk_signal_thresholds_match_python() {
        assert_eq!(de_signal(None), "no data");
        assert_eq!(de_signal(Some(-1.0)), "negative equity");
        assert_eq!(de_signal(Some(49.9)), "conservative");
        assert_eq!(de_signal(Some(99.9)), "moderate");
        assert_eq!(de_signal(Some(199.9)), "leveraged");
        assert_eq!(de_signal(Some(200.0)), "highly leveraged");

        assert_eq!(cr_signal(None), "no data");
        assert_eq!(cr_signal(Some(2.0)), "strong");
        assert_eq!(cr_signal(Some(1.5)), "adequate");
        assert_eq!(cr_signal(Some(1.0)), "tight");
        assert_eq!(cr_signal(Some(0.99)), "weak");
    }

    #[test]
    fn report_overalls_match_python_logic() {
        let fundamentals = sample_fundamentals();

        let growth = analyze_growth(&fundamentals);
        assert_eq!(growth.overall_signal, "growing");

        let valuation = analyze_valuation(&fundamentals);
        assert_eq!(valuation.overall_signal, "undervalued");
        assert_eq!(valuation.ev_ebitda, Some(8.0));

        let risk = analyze_risk(&fundamentals);
        assert_eq!(risk.overall_signal, "low risk");

        let fundamental = analyze_fundamental("BBCA.JK", &fundamentals);
        assert_eq!(fundamental.overall_signal, "healthy");
    }

    #[test]
    fn incomplete_growth_and_high_risk_paths_match_python_logic() {
        let mut fundamentals = sample_fundamentals();
        fundamentals.revenue_growth = None;
        fundamentals.earnings_growth = Some(0.05);
        fundamentals.debt_to_equity = Some(250.0);
        fundamentals.current_ratio = Some(0.9);

        let growth = analyze_growth(&fundamentals);
        assert_eq!(growth.overall_signal, "incomplete data");

        let risk = analyze_risk(&fundamentals);
        assert_eq!(risk.overall_signal, "high risk");
    }
}
