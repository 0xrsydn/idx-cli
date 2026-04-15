use std::collections::HashMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MsnQuote {
    #[serde(default)]
    pub(crate) symbol: Option<String>,
    #[serde(default)]
    pub(crate) short_name: Option<String>,
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
    #[serde(default)]
    pub(crate) return_ytd: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyRatios {
    #[allow(dead_code)]
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
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) revenue_growth_rate: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) earnings_growth_rate: Option<f64>,
    #[serde(
        default,
        rename = "netIncomeYTDYTDGrowthRate",
        deserialize_with = "de_opt_f64_lenient"
    )]
    pub(crate) net_income_ytd_ytd_growth_rate: Option<f64>,
    #[serde(
        default,
        rename = "revenueYTDYTD",
        deserialize_with = "de_opt_f64_lenient"
    )]
    pub(crate) revenue_ytd_ytd: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) net_margin: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) profit_margin: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) roe: Option<f64>,
    #[serde(default, rename = "roaTTM", deserialize_with = "de_opt_f64_lenient")]
    pub(crate) roa_ttm: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) return_on_asset_current: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) debt_to_equity_ratio: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) current_ratio: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) price_to_earnings_ratio: Option<f64>,
    #[serde(
        default,
        rename = "forwardPriceToEPS",
        deserialize_with = "de_opt_f64_lenient"
    )]
    pub(crate) forward_price_to_eps: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    pub(crate) price_to_book_ratio: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawEquity {
    pub(super) id: Option<String>,
    pub(super) symbol: Option<String>,
    pub(super) short_name: Option<String>,
    pub(super) long_name: Option<String>,
    pub(super) display_name: Option<String>,
    pub(super) description: Option<String>,
    pub(super) sector: Option<String>,
    pub(super) industry: Option<String>,
    pub(super) website: Option<String>,
    pub(super) full_time_employees: Option<i64>,
    pub(super) address: Option<String>,
    pub(super) city: Option<String>,
    pub(super) country: Option<String>,
    pub(super) phone: Option<String>,
    pub(super) officers: Option<Vec<RawOfficer>>,
    pub(super) company: Option<RawCompany>,
    pub(super) localized_attributes: Option<HashMap<String, RawLocalizedAttribute>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawOfficer {
    pub(super) name: Option<String>,
    pub(super) title: Option<String>,
    pub(super) age: Option<i32>,
    pub(super) year_born: Option<i32>,
    pub(super) total_pay: Option<i64>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawCompany {
    pub(super) address: Option<RawCompanyAddress>,
    pub(super) description: Option<String>,
    pub(super) employees: Option<i64>,
    pub(super) industry: Option<String>,
    pub(super) sector: Option<String>,
    pub(super) website: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawCompanyAddress {
    pub(super) street: Option<String>,
    pub(super) city: Option<String>,
    pub(super) country: Option<String>,
    pub(super) phone: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawLocalizedAttribute {
    pub(super) display_name: Option<String>,
    pub(super) description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawFinancialStatement {
    pub(super) underlying_instrument: Option<RawInstrumentInfo>,
    pub(super) balance_sheets: Option<RawStatementSection>,
    pub(super) cash_flow: Option<RawStatementSection>,
    #[serde(rename = "incomeStatement")]
    pub(super) income_statements: Option<RawStatementSection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawInstrumentInfo {
    pub(super) instrument_id: Option<String>,
    pub(super) display_name: Option<String>,
    pub(super) short_name: Option<String>,
    pub(super) symbol: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct RawStatementSection {
    #[serde(flatten)]
    pub(super) data: HashMap<String, serde_json::Value>,
    pub(super) currency: Option<String>,
    pub(super) source: Option<String>,
    #[serde(rename = "sourceDate")]
    pub(super) source_date: Option<String>,
    #[serde(rename = "reportDate")]
    pub(super) report_date: Option<String>,
    #[serde(rename = "endDate")]
    pub(super) end_date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawEarningsResponse {
    pub(super) eps_last_year: Option<f64>,
    pub(super) revenue_last_year: Option<f64>,
    pub(super) forecast: Option<RawEarningsBucket>,
    pub(super) history: Option<RawEarningsBucket>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawEarningsBucket {
    pub(super) annual: Option<HashMap<String, RawEarningsData>>,
    pub(super) quarterly: Option<HashMap<String, RawEarningsData>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawEarningsData {
    pub(super) eps_actual: Option<f64>,
    pub(super) eps_surprise: Option<f64>,
    pub(super) eps_surprise_percent: Option<f64>,
    pub(super) eps_forecast: Option<f64>,
    pub(super) revenue_actual: Option<f64>,
    pub(super) revenue_surprise: Option<f64>,
    pub(super) revenue_forecast: Option<f64>,
    pub(super) earning_release_date: Option<String>,
    pub(super) ciq_fiscal_period_type: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawSentiment {
    pub(super) symbol: Option<String>,
    pub(super) display_name: Option<String>,
    pub(super) sentiment_statistics: Option<Vec<RawSentimentStat>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawSentimentStat {
    pub(super) time_range_name: Option<String>,
    pub(super) bullish: Option<i32>,
    pub(super) bearish: Option<i32>,
    pub(super) neutral: Option<i32>,
}

// Actual MSN insights API response: array of insight containers, each holding
// individual insight items grouped by category (Valuation, Risk, etc.)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawInsight {
    pub(super) instrument_id: Option<String>,
    pub(super) display_name: Option<String>,
    pub(super) insights: Option<Vec<RawInsightItem>>,
    pub(super) time_last_updated: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawInsightItem {
    pub(super) insight_name: Option<String>,
    pub(super) category: Option<String>,
    pub(super) insight_statement: Option<String>,
    pub(super) short_insight_statement: Option<String>,
    pub(super) details: Option<RawInsightDetails>,
    pub(super) time_last_updated: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawInsightDetails {
    pub(super) evaluation_status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawNewsFeed {
    pub(super) value: Option<Vec<RawNewsItem>>,
    #[serde(rename = "subCards")]
    pub(super) sub_cards: Option<Vec<RawNewsItem>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawNewsItem {
    pub(super) id: Option<String>,
    pub(super) title: Option<String>,
    pub(super) url: Option<String>,
    #[serde(rename = "abstract")]
    pub(super) description: Option<String>,
    pub(super) provider: Option<RawNewsProvider>,
    pub(super) published_date_time: Option<String>,
    pub(super) read_time_min: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawNewsProvider {
    pub(super) name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ScreenerRequest {
    pub(super) filter: Vec<ScreenerFilter>,
    pub(super) order: ScreenerOrder,
    pub(super) return_value_type: Vec<String>,
    pub(super) screener_type: String,
    pub(super) limit: usize,
    pub(super) page_index: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ScreenerFilter {
    pub(super) key: String,
    pub(super) key_group: String,
    pub(super) is_range: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct ScreenerOrder {
    pub(super) key: String,
    pub(super) dir: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawScreenerResponse {
    pub(super) count: Option<i32>,
    pub(super) quote: Option<Vec<MsnQuote>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawChartResponse {
    #[serde(rename = "_p")]
    pub(super) id: Option<String>,
    pub(super) chart_type: Option<String>,
    pub(super) symbol: Option<String>,
    pub(super) series: Option<RawChartSeries>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawChartSeries {
    #[serde(default)]
    pub(super) time_stamps: Vec<String>,
    #[serde(default)]
    pub(super) prices: Vec<Option<f64>>,
    #[serde(default)]
    pub(super) open_prices: Vec<Option<f64>>,
    #[serde(default)]
    pub(super) prices_high: Vec<Option<f64>>,
    #[serde(default)]
    pub(super) prices_low: Vec<Option<f64>>,
    #[serde(default)]
    pub(super) volumes: Vec<Option<f64>>,
    pub(super) start_time: Option<String>,
    pub(super) end_time: Option<String>,
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
            if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("nan")
                || trimmed.eq_ignore_ascii_case("infinity")
                || trimmed.eq_ignore_ascii_case("-infinity")
            {
                Ok(None)
            } else {
                trimmed
                    .parse::<f64>()
                    .map_err(D::Error::custom)
                    .map(|number| {
                        if number.is_finite() {
                            Some(number)
                        } else {
                            None
                        }
                    })
            }
        }
        None => Ok(None),
    }
}
