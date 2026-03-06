// Raw serde structs for Yahoo API responses. Fields not yet consumed by map.rs are
// retained for future fundamentals expansion; suppress dead_code for forward-compat.
#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct ChartResponse {
    pub(super) chart: ChartRoot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QuoteSummaryResponse {
    pub(super) quote_summary: QuoteSummaryRoot,
}

#[derive(Debug, Deserialize)]
pub(super) struct QuoteSummaryRoot {
    pub(super) result: Option<Vec<QuoteSummaryResult>>,
    pub(super) error: Option<ChartError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QuoteSummaryResult {
    #[serde(default)]
    pub(super) summary_detail: Option<SummaryDetail>,
    #[serde(default)]
    pub(super) default_key_statistics: Option<DefaultKeyStatistics>,
    #[serde(default)]
    pub(super) financial_data: Option<FinancialData>,
    #[serde(default)]
    pub(super) asset_profile: Option<AssetProfile>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChartRoot {
    pub(super) result: Option<Vec<ChartResult>>,
    pub(super) error: Option<ChartError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChartError {
    pub(super) code: String,
    pub(super) description: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChartResult {
    pub(super) meta: Option<ChartMeta>,
    pub(super) timestamp: Option<Vec<i64>>,
    pub(super) indicators: Option<Indicators>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(super) struct ChartMeta {
    pub(super) symbol: Option<String>,
    pub(super) regular_market_price: Option<f64>,
    pub(super) previous_close: Option<f64>,
    pub(super) chart_previous_close: Option<f64>,
    pub(super) regular_market_volume: Option<u64>,
    pub(super) regular_market_day_high: Option<f64>,
    pub(super) regular_market_day_low: Option<f64>,
    pub(super) market_cap: Option<u64>,
    pub(super) fifty_two_week_high: Option<f64>,
    pub(super) fifty_two_week_low: Option<f64>,
    #[serde(rename = "averageDailyVolume3Month")]
    pub(super) average_daily_volume_3month: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Indicators {
    pub(super) quote: Option<Vec<IndicatorQuote>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct IndicatorQuote {
    pub(super) open: Option<Vec<Option<f64>>>,
    pub(super) high: Option<Vec<Option<f64>>>,
    pub(super) low: Option<Vec<Option<f64>>>,
    pub(super) close: Option<Vec<Option<f64>>>,
    pub(super) volume: Option<Vec<Option<u64>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryDetail {
    #[serde(rename = "trailingPE")]
    pub trailing_pe: Option<FloatValue>,
    #[serde(rename = "forwardPE")]
    pub forward_pe: Option<FloatValue>,
    pub price_to_book: Option<FloatValue>,
    pub dividend_yield: Option<FloatValue>,
    pub market_cap: Option<FloatValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultKeyStatistics {
    #[serde(rename = "trailingPE")]
    pub trailing_pe: Option<FloatValue>,
    #[serde(rename = "forwardPE")]
    pub forward_pe: Option<FloatValue>,
    pub price_to_book: Option<FloatValue>,
    pub earnings_growth: Option<FloatValue>,
    pub enterprise_value: Option<IntValue>,
    pub ebitda: Option<IntValue>,
    pub market_cap: Option<UIntValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinancialData {
    #[serde(rename = "trailingPE")]
    pub trailing_pe: Option<FloatValue>,
    #[serde(rename = "forwardPE")]
    pub forward_pe: Option<FloatValue>,
    pub price_to_book: Option<FloatValue>,
    pub return_on_equity: Option<FloatValue>,
    pub profit_margins: Option<FloatValue>,
    pub return_on_assets: Option<FloatValue>,
    pub revenue_growth: Option<FloatValue>,
    pub earnings_growth: Option<FloatValue>,
    pub debt_to_equity: Option<FloatValue>,
    pub current_ratio: Option<FloatValue>,
    pub enterprise_value: Option<IntValue>,
    pub ebitda: Option<IntValue>,
    pub market_cap: Option<UIntValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetProfile {
    pub sector: Option<String>,
    pub industry: Option<String>,
    pub long_business_summary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FloatValue {
    pub raw: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct IntValue {
    pub raw: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UIntValue {
    pub raw: Option<u64>,
}
