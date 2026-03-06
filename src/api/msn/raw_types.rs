use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MsnQuote {
    #[serde(default)]
    pub(crate) symbol: Option<String>,
    pub(crate) price: Option<f64>,
    #[serde(default)]
    pub(crate) price_change: Option<f64>,
    #[serde(default)]
    pub(crate) price_change_percent: Option<f64>,
    #[serde(default)]
    pub(crate) price_previous_close: Option<f64>,
    #[serde(default, rename = "price52wHigh")]
    pub(crate) price_52w_high: Option<f64>,
    #[serde(default, rename = "price52wLow")]
    pub(crate) price_52w_low: Option<f64>,
    #[serde(default)]
    pub(crate) accumulated_volume: Option<f64>,
    #[serde(default)]
    pub(crate) average_volume: Option<f64>,
    #[serde(default)]
    pub(crate) market_cap: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyRatios {
    #[serde(default)]
    pub(crate) industry_metrics: Vec<IndustryMetric>,
    #[serde(default)]
    pub(crate) company_metrics: Vec<IndustryMetric>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IndustryMetric {
    pub(crate) year: Option<String>,
    pub(crate) fiscal_period_type: Option<String>,
    #[serde(default)]
    pub(crate) revenue_growth_rate: Option<f64>,
    #[serde(default)]
    pub(crate) earnings_growth_rate: Option<f64>,
    #[serde(default, rename = "netIncomeYTDYTDGrowthRate")]
    pub(crate) net_income_ytd_ytd_growth_rate: Option<f64>,
    #[serde(default, rename = "revenueYTDYTD")]
    pub(crate) revenue_ytd_ytd: Option<f64>,
    #[serde(default)]
    pub(crate) net_margin: Option<f64>,
    #[serde(default)]
    pub(crate) profit_margin: Option<f64>,
    #[serde(default)]
    pub(crate) roe: Option<f64>,
    #[serde(default, rename = "roaTTM")]
    pub(crate) roa_ttm: Option<f64>,
    #[serde(default)]
    pub(crate) return_on_asset_current: Option<f64>,
    #[serde(default)]
    pub(crate) debt_to_equity_ratio: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) current_ratio: Option<f64>,
    #[serde(default)]
    pub(crate) price_to_earnings_ratio: Option<f64>,
    #[serde(default, rename = "forwardPriceToEPS")]
    pub(crate) forward_price_to_eps: Option<f64>,
    #[serde(default)]
    pub(crate) price_to_book_ratio: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MsnChart {
    pub(crate) series: ChartSeries,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChartSeries {
    #[serde(default)]
    pub(crate) time_stamps: Vec<String>,
    #[serde(default)]
    pub(crate) prices: Vec<f64>,
    #[serde(default)]
    pub(crate) open_prices: Vec<f64>,
    #[serde(default)]
    pub(crate) prices_high: Vec<f64>,
    #[serde(default)]
    pub(crate) prices_low: Vec<f64>,
    #[serde(default)]
    pub(crate) volumes: Vec<f64>,
}

impl ChartSeries {
    pub(crate) fn has_real_ohlcv(&self) -> bool {
        !self.time_stamps.is_empty()
            && self.open_prices.len() == self.time_stamps.len()
            && self.prices_high.len() == self.time_stamps.len()
            && self.prices_low.len() == self.time_stamps.len()
            && self.prices.len() == self.time_stamps.len()
            && self.volumes.len() == self.time_stamps.len()
    }
}

fn de_opt_f64_lenient<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumberLike {
        F64(f64),
        String(String),
    }

    let value = Option::<NumberLike>::deserialize(deserializer)?;
    match value {
        Some(NumberLike::F64(number)) if number.is_finite() => Ok(Some(number)),
        Some(NumberLike::F64(_)) => Ok(None),
        Some(NumberLike::String(raw)) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("nan") {
                Ok(None)
            } else {
                trimmed.parse::<f64>().map(Some).map_err(D::Error::custom)
            }
        }
        None => Ok(None),
    }
}
