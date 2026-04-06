use chrono::NaiveDate;
use clap::ValueEnum;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Snapshot quote data normalized from Yahoo Finance `/v8/finance/chart` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    /// Trading symbol as returned by Yahoo `chart.result[0].meta.symbol`.
    pub symbol: String,
    /// Last traded regular market price in IDR (whole Rupiah), mapped from
    /// `chart.result[0].meta.regularMarketPrice` and rounded to nearest integer.
    #[serde(deserialize_with = "de_i64_from_number")]
    pub price: i64,
    /// Absolute day change in IDR (whole Rupiah), computed as
    /// `regularMarketPrice - previousClose` using rounded integer prices.
    #[serde(deserialize_with = "de_i64_from_number")]
    pub change: i64,
    /// Percentage day change as decimal percent (`0-100` scale), computed from
    /// Yahoo `regularMarketPrice` and `previousClose` raw floats.
    pub change_pct: f64,
    /// Traded regular market volume (shares), from `regularMarketVolume`.
    pub volume: u64,
    /// Company market capitalization in IDR, from `marketCap`.
    #[serde(default, deserialize_with = "de_opt_u64_from_number")]
    pub market_cap: Option<u64>,
    /// 52-week high in IDR (whole Rupiah), from `fiftyTwoWeekHigh` rounded.
    #[serde(default, deserialize_with = "de_opt_i64_from_number")]
    pub week52_high: Option<i64>,
    /// 52-week low in IDR (whole Rupiah), from `fiftyTwoWeekLow` rounded.
    #[serde(default, deserialize_with = "de_opt_i64_from_number")]
    pub week52_low: Option<i64>,
    /// Relative position within 52-week range (`0.0..=1.0`), computed from raw
    /// Yahoo `fiftyTwoWeekLow` and `fiftyTwoWeekHigh`.
    pub week52_position: Option<f64>,
    /// Coarse 52-week range bucket derived from `week52_position`.
    pub range_signal: Option<String>,
    /// Previous close in IDR (whole Rupiah), from
    /// `previousClose` or `chartPreviousClose`, rounded.
    #[serde(default, deserialize_with = "de_opt_i64_from_number")]
    pub prev_close: Option<i64>,
    /// Average daily volume for the last 3 months (shares), from
    /// `averageDailyVolume3Month`.
    pub avg_volume: Option<u64>,
}

/// OHLC candle data normalized from Yahoo Finance chart indicators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ohlc {
    /// Candle date (exchange-local day boundary from Yahoo timestamp).
    pub date: NaiveDate,
    /// Opening price in IDR (whole Rupiah), from `indicators.quote[0].open` rounded.
    #[serde(deserialize_with = "de_i64_from_number")]
    pub open: i64,
    /// Highest traded price in IDR (whole Rupiah), from `indicators.quote[0].high` rounded.
    #[serde(deserialize_with = "de_i64_from_number")]
    pub high: i64,
    /// Lowest traded price in IDR (whole Rupiah), from `indicators.quote[0].low` rounded.
    #[serde(deserialize_with = "de_i64_from_number")]
    pub low: i64,
    /// Closing price in IDR (whole Rupiah), from `indicators.quote[0].close` rounded.
    #[serde(deserialize_with = "de_i64_from_number")]
    pub close: i64,
    /// Traded volume (shares), from `indicators.quote[0].volume`.
    pub volume: u64,
}

/// Fundamental metrics normalized from Yahoo Finance `/v10/finance/quoteSummary`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fundamentals {
    pub trailing_pe: Option<f64>,
    pub forward_pe: Option<f64>,
    pub price_to_book: Option<f64>,
    pub return_on_equity: Option<f64>,
    pub profit_margins: Option<f64>,
    pub return_on_assets: Option<f64>,
    pub revenue_growth: Option<f64>,
    pub earnings_growth: Option<f64>,
    pub debt_to_equity: Option<f64>,
    pub current_ratio: Option<f64>,
    pub enterprise_value: Option<i64>,
    pub ebitda: Option<i64>,
    pub market_cap: Option<u64>,
}

pub type Bar = Ohlc;

// Forward-looking types for planned MSN endpoints — used once capability traits are wired to CLI.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyProfile {
    pub id: String,
    pub symbol: String,
    pub short_name: String,
    pub long_name: String,
    pub description: String,
    pub sector: String,
    pub industry: String,
    pub website: String,
    pub employees: i64,
    pub address: String,
    pub city: String,
    pub country: String,
    pub phone: String,
    pub officers: Vec<Officer>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Officer {
    pub name: String,
    pub title: String,
    pub age: Option<i32>,
    pub year_born: Option<i32>,
    pub total_pay: Option<i64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialStatements {
    pub instrument: InstrumentInfo,
    pub balance_sheet: Option<StatementSection>,
    pub cash_flow: Option<StatementSection>,
    pub income_statement: Option<StatementSection>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentInfo {
    pub id: String,
    pub symbol: String,
    pub name: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatementSection {
    pub values: std::collections::HashMap<String, f64>,
    pub currency: String,
    pub report_date: String,
    pub end_date: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsReport {
    #[serde(default)]
    pub symbol: String,
    pub eps_last_year: f64,
    pub revenue_last_year: f64,
    pub forecast: Vec<EarningsData>,
    pub history: Vec<EarningsData>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsData {
    pub eps_actual: Option<f64>,
    pub eps_forecast: Option<f64>,
    pub eps_surprise: Option<f64>,
    pub eps_surprise_pct: Option<f64>,
    pub revenue_actual: Option<f64>,
    pub revenue_forecast: Option<f64>,
    pub revenue_surprise: Option<f64>,
    pub earning_release_date: Option<String>,
    pub period_type: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentData {
    pub symbol: String,
    pub statistics: Vec<SentimentPeriod>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentPeriod {
    pub time_range: String,
    pub bullish: i32,
    pub bearish: i32,
    pub neutral: i32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightData {
    pub id: String,
    #[serde(default)]
    pub symbol: String,
    pub summary: String,
    pub highlights: Vec<String>,
    pub risks: Vec<String>,
    pub last_updated: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub id: String,
    #[serde(default)]
    pub symbol: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub provider: String,
    pub published_at: String,
    pub read_time_min: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NumberLike {
    I64(i64),
    U64(u64),
    F64(f64),
}

fn de_i64_from_number<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = NumberLike::deserialize(deserializer)?;
    Ok(match value {
        NumberLike::I64(v) => v,
        NumberLike::U64(v) => i64::try_from(v).map_err(D::Error::custom)?,
        NumberLike::F64(v) => v.round() as i64,
    })
}

fn de_opt_i64_from_number<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<NumberLike>::deserialize(deserializer).and_then(|v| {
        v.map(|n| match n {
            NumberLike::I64(x) => Ok(x),
            NumberLike::U64(x) => i64::try_from(x).map_err(D::Error::custom),
            NumberLike::F64(x) => Ok(x.round() as i64),
        })
        .transpose()
    })
}

fn de_opt_u64_from_number<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<NumberLike>::deserialize(deserializer).and_then(|v| {
        v.map(|n| match n {
            NumberLike::I64(x) => u64::try_from(x).map_err(D::Error::custom),
            NumberLike::U64(x) => Ok(x),
            NumberLike::F64(x) => {
                if x.is_sign_negative() {
                    Err(D::Error::custom(
                        "negative value cannot be converted to u64",
                    ))
                } else {
                    Ok(x.round() as u64)
                }
            }
        })
        .transpose()
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, ValueEnum)]
pub enum Period {
    #[value(name = "1d")]
    OneDay,
    #[value(name = "5d")]
    FiveDays,
    #[value(name = "1mo")]
    OneMonth,
    #[value(name = "3mo")]
    ThreeMonths,
    #[value(name = "6mo")]
    SixMonths,
    #[value(name = "1y")]
    OneYear,
    #[value(name = "2y")]
    TwoYears,
    #[value(name = "5y")]
    FiveYears,
}

impl Period {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OneDay => "1d",
            Self::FiveDays => "5d",
            Self::OneMonth => "1mo",
            Self::ThreeMonths => "3mo",
            Self::SixMonths => "6mo",
            Self::OneYear => "1y",
            Self::TwoYears => "2y",
            Self::FiveYears => "5y",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ValueEnum)]
pub enum Interval {
    #[value(name = "1d")]
    Day,
    #[value(name = "1wk")]
    Week,
    #[value(name = "1mo")]
    Month,
}

impl Interval {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "1wk",
            Self::Month => "1mo",
        }
    }
}
