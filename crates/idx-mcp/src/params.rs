//! Tool inputs. String enums are validated by idx-core's `FromStr`, so the
//! accepted values and error messages match the CLI; the schemas list them.

use idx_core::analysis::bandar::TIERS;
use idx_core::api::types::{Interval, Period};
use idx_core::broker::types::{BrokerBoard, BrokerInvestor};
use idx_core::corporate::types::ActionKind;
use idx_core::flow::types::FlowSource;
use idx_core::market::{ScreenFilter, ScreenRegion};
use idx_core::movements::types::{ActionFilter, MAX_PAGES};
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
pub struct IntradayParams {
    /// IDX ticker, e.g. "BBCA"; indices as "^JKSE" or "IHSG".
    pub symbol: String,
    /// Only the latest N minutes (default: the whole session, at most 400).
    #[serde(default)]
    pub tail: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OrderBookParams {
    /// IDX ticker, e.g. "BBCA".
    pub symbol: String,
    /// Price levels per side (default 10, at most 50).
    #[serde(default)]
    pub depth: Option<usize>,
}

/// How `stockbit_login` signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoginMethod {
    Auto,
    Browser,
    Page,
}

impl LoginMethod {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Browser, Self::Page];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Browser => "browser",
            Self::Page => "page",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LoginParams {
    /// "auto" (default): a browser window when idx-mcp's machine has a display and no SSH, else the local page; "browser" or "page" picks one.
    #[serde(default)]
    #[schemars(schema_with = "login_method_schema")]
    pub method: Option<LoginMethod>,
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
    #[schemars(schema_with = "top_schema")]
    pub top: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FlowParams {
    /// IDX ticker, e.g. "BBCA".
    pub symbol: String,
    /// Latest sessions (default 10, at most 60). Omit when giving from/to.
    #[serde(default)]
    pub days: Option<usize>,
    /// First session of a range, YYYY-MM-DD (at most 92 days).
    #[serde(default)]
    pub from: Option<String>,
    /// Last session of the range, YYYY-MM-DD (default today).
    #[serde(default)]
    pub to: Option<String>,
    /// Where the figures come from (default "ipot"): "ipot" gives foreign buy, sell and net
    /// value across all boards; "idx" gives exact share counts, the close and the non-regular
    /// share, but needs curl-impersonate and may be blocked (HTTP 403).
    #[serde(default)]
    #[schemars(schema_with = "flow_source_schema")]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CorporateActionsParams {
    /// IDX ticker, e.g. "BBCA".
    pub symbol: String,
    /// Kinds to list (default: every kind but "ipo"). Each kind is one request to Stockbit.
    #[serde(default)]
    #[schemars(schema_with = "action_kinds_schema")]
    pub kinds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CorporateCalendarParams {
    /// First day, YYYY-MM-DD (default today).
    #[serde(default)]
    pub from: Option<String>,
    /// Last day, YYYY-MM-DD (default 30 days after `from`; at most 366 days).
    #[serde(default)]
    pub to: Option<String>,
    /// Kinds to list (default: every kind). Each kind is one request to Stockbit.
    #[serde(default)]
    #[schemars(schema_with = "action_kinds_schema")]
    pub kinds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MovementsParams {
    /// IDX ticker, e.g. "BBCA"; omit for the whole market.
    #[serde(default)]
    pub symbol: Option<String>,
    /// First day, YYYY-MM-DD.
    #[serde(default)]
    pub from: Option<String>,
    /// Last day, YYYY-MM-DD.
    #[serde(default)]
    pub to: Option<String>,
    /// One holder's movements: the `holder_id` of a row.
    #[serde(default)]
    pub holder_id: Option<String>,
    /// Only this kind of movement; "corporate-action" covers rights issues, private placements and the like.
    #[serde(default)]
    #[schemars(schema_with = "movement_action_schema")]
    pub action: Option<String>,
    /// "idx" (insider disclosures, any size) or "ksei" (holders at 5% and above).
    #[serde(default)]
    #[schemars(schema_with = "movement_source_schema")]
    pub source: Option<String>,
    /// Pages of 50 rows (default 1, at most 10); each page is one request to Stockbit.
    #[serde(default)]
    #[schemars(schema_with = "movement_pages_schema")]
    pub pages: Option<u32>,
}

/// Parses `kinds`, or `default` when absent.
pub fn action_kinds(
    kinds: Option<Vec<String>>,
    default: &[ActionKind],
) -> Result<Vec<ActionKind>, idx_core::error::IdxError> {
    match kinds {
        None => Ok(default.to_vec()),
        Some(kinds) => kinds.iter().map(|k| k.parse()).collect(),
    }
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

fn flow_source_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(FlowSource::ALL.map(FlowSource::as_str).to_vec())
}

fn action_kinds_schema(_: &mut SchemaGenerator) -> Schema {
    let kinds: Vec<&str> = ActionKind::ALL.iter().map(|k| k.as_str()).collect();
    json_schema!({ "type": "array", "items": { "type": "string", "enum": kinds } })
}

fn movement_action_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(ActionFilter::ALL.map(ActionFilter::as_str).to_vec())
}

fn movement_source_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(vec!["idx", "ksei"])
}

fn movement_pages_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "minimum": 1, "maximum": MAX_PAGES })
}

fn login_method_schema(_: &mut SchemaGenerator) -> Schema {
    string_enum(LoginMethod::ALL.map(LoginMethod::as_str).to_vec())
}

fn top_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "enum": TIERS })
}
