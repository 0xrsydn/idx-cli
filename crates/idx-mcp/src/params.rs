//! Tool inputs. String enums are validated by idx-core's `FromStr`, so the
//! accepted values and error messages match the CLI; the schemas list them.

use idx_core::api::types::{Interval, Period};
use idx_core::broker::types::{BrokerBoard, BrokerInvestor};
use idx_core::market::{ScreenFilter, ScreenRegion};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolsParams {
    /// IDX tickers, e.g. ["BBCA", "BBRI"]; the `.JK` suffix is optional. At most 20.
    pub symbols: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HistoryParams {
    /// IDX ticker, e.g. "BBCA".
    pub symbol: String,
    /// Lookback window (default "3mo").
    #[serde(default)]
    #[schemars(schema_with = "period_schema")]
    pub period: Option<String>,
    /// Candle size (default "1d").
    #[serde(default)]
    #[schemars(schema_with = "interval_schema")]
    pub interval: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScreenParams {
    /// Screen to run (default "top-performers").
    #[serde(default)]
    #[schemars(schema_with = "filter_schema")]
    pub filter: Option<String>,
    /// Market region (default "id").
    #[serde(default)]
    #[schemars(schema_with = "region_schema")]
    pub region: Option<String>,
    /// Maximum results (default 50, at most 200).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CompanySection {
    Profile,
    Financials,
    Earnings,
    Sentiment,
    Insights,
    News,
}

impl CompanySection {
    pub const ALL: [Self; 6] = [
        Self::Profile,
        Self::Financials,
        Self::Earnings,
        Self::Sentiment,
        Self::Insights,
        Self::News,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Financials => "financials",
            Self::Earnings => "earnings",
            Self::Sentiment => "sentiment",
            Self::Insights => "insights",
            Self::News => "news",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompanyParams {
    /// IDX ticker, e.g. "BBCA".
    pub symbol: String,
    /// Sections to fetch (default: all).
    #[serde(default)]
    pub sections: Option<Vec<CompanySection>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisKind {
    Technical,
    Growth,
    Valuation,
    Risk,
}

impl AnalysisKind {
    pub const ALL: [Self; 4] = [Self::Technical, Self::Growth, Self::Valuation, Self::Risk];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Technical => "technical",
            Self::Growth => "growth",
            Self::Valuation => "valuation",
            Self::Risk => "risk",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeParams {
    /// IDX ticker, e.g. "BBCA".
    pub symbol: String,
    /// Analyses to run (default: all).
    #[serde(default)]
    pub kinds: Option<Vec<AnalysisKind>>,
}

/// Which sessions and trades a broker tool covers.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrokerScope {
    /// One session, YYYY-MM-DD. Omit date, from and to for the latest session with trades.
    #[serde(default)]
    pub date: Option<String>,
    /// First session of a range, YYYY-MM-DD (at most 366 days).
    #[serde(default)]
    pub from: Option<String>,
    /// Last session of the range, YYYY-MM-DD (default today).
    #[serde(default)]
    pub to: Option<String>,
    /// Whose trades to count, by investor domicile (default "all").
    #[serde(default)]
    #[schemars(schema_with = "investor_schema")]
    pub investor: Option<String>,
    /// Which board's trades to count (default "all"); "regular" leaves out
    /// negotiated block trades.
    #[serde(default)]
    #[schemars(schema_with = "board_schema")]
    pub board: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrokerParams {
    /// IDX ticker, e.g. "BBCA".
    pub symbol: String,
    #[serde(flatten)]
    pub scope: BrokerScope,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BandarParams {
    /// IDX tickers, e.g. ["BBCA", "BBRI"]; the `.JK` suffix is optional. At most 20.
    pub symbols: Vec<String>,
    #[serde(flatten)]
    pub scope: BrokerScope,
    /// Tier behind the headline signal: 1, 3, 5 or 10 brokers per side (default 5).
    #[serde(default)]
    pub top: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShareholdersParams {
    /// IDX ticker, e.g. "BBCA".
    pub ticker: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EntityParams {
    /// Investor name or part of it, e.g. "DWIMURIA"; the closest match is used.
    pub entity: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Full-text query over investor names.
    pub query: String,
    /// Maximum results (default 20, at most 100).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChangesParams {
    /// Earlier release date, YYYY-MM-DD (see ownership_status).
    pub from: String,
    /// Later release date, YYYY-MM-DD.
    pub to: String,
    /// Only this ticker's changes.
    #[serde(default)]
    pub ticker: Option<String>,
    /// Rows per page (default 200, at most 300); `total` reports the full count.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Rows to skip, for the next page: the previous response's `next_offset`.
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncParams {
    /// Re-download even when the local snapshot is current.
    #[serde(default)]
    pub force: bool,
}

fn string_enum(values: Vec<&'static str>) -> Schema {
    json_schema!({ "type": "string", "enum": values })
}

fn period_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(Period::ALL.iter().map(Period::as_str).collect())
}

fn interval_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(Interval::ALL.iter().map(Interval::as_str).collect())
}

fn filter_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(ScreenFilter::ALL.map(ScreenFilter::as_str).to_vec())
}

fn region_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(ScreenRegion::ALL.map(ScreenRegion::as_str).to_vec())
}

fn investor_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(BrokerInvestor::ALL.map(BrokerInvestor::as_str).to_vec())
}

fn board_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(BrokerBoard::ALL.map(BrokerBoard::as_str).to_vec())
}
