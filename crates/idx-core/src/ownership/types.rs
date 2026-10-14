use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Which data source a holding originates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OwnershipSource {
    /// KSEI shareholder registry source.
    Ksei,
    /// Bing institutional ownership source.
    Bing,
}

/// Investor locality classification from KSEI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locality {
    /// Local Indonesian investor.
    Local,
    /// Foreign investor.
    Foreign,
}

/// Bing institutional flow signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowSignal {
    /// Existing top holder.
    Holder,
    /// Net buyer for the period.
    Buyer,
    /// Net seller for the period.
    Seller,
    /// New position opened this period.
    NewPosition,
    /// Fully exited this period.
    Exited,
}

/// Method used to resolve an entity alias to a canonical entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolutionMethod {
    /// Exact string match after normalization.
    Exact,
    /// Rule-based normalization match.
    Rule,
    /// Fuzzy or similarity-based match.
    Fuzzy,
    /// Human-curated manual mapping.
    Manual,
}

/// Graph node category used in ownership network output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphNodeType {
    /// Canonical investor entity node.
    Entity,
    /// Listed issuer ticker node.
    Ticker,
}

/// KSEI investor type code (for example: `CP`, `ID`, `IB`, `MF`, `SC`, `IS`, `OT`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvestorTypeCode(pub String);

/// Canonical resolved entity (investor or shareholder).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Internal entity identifier.
    pub id: i64,
    /// Canonical normalized display name.
    pub canonical_name: String,
    /// Optional coarse entity type (`fund`, `bank`, `conglomerate`, `govt`, `individual`).
    pub entity_type: Option<String>,
    /// Optional ISO-like country tag.
    pub country: Option<String>,
}

/// A raw name variant mapped to a canonical entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityAlias {
    /// Internal alias identifier.
    pub id: i64,
    /// Referenced canonical entity id.
    pub entity_id: i64,
    /// Raw alias text from source.
    pub raw_name: String,
    /// Source that produced this alias.
    pub source: OwnershipSource,
    /// Resolution confidence score in `0.0..=1.0`.
    pub confidence: f64,
    /// Matching method used for resolution.
    pub method: ResolutionMethod,
}

/// Ticker (issuer) reference metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker {
    /// Internal ticker identifier.
    pub id: i64,
    /// Exchange ticker code, for example `BBCA`.
    pub code: String,
    /// Optional long issuer name.
    pub name: Option<String>,
    /// Optional sector classification.
    pub sector: Option<String>,
}

/// Single KSEI ownership row for one holder in one ticker and release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KseiHolding {
    /// Internal holding row identifier.
    pub id: i64,
    /// Referenced ticker id.
    pub ticker_id: i64,
    /// Referenced entity id, unresolved when `None`.
    pub entity_id: Option<i64>,
    /// Raw investor name exactly as imported.
    pub raw_investor_name: String,
    /// Optional KSEI investor type code.
    pub investor_type: Option<InvestorTypeCode>,
    /// Optional local/foreign classification.
    pub locality: Option<Locality>,
    /// Optional nationality text.
    pub nationality: Option<String>,
    /// Optional domicile text.
    pub domicile: Option<String>,
    /// Scripless holdings in shares.
    pub holdings_scripless: i64,
    /// Script holdings in shares.
    pub holdings_scrip: i64,
    /// Total shares held.
    pub total_shares: i64,
    /// Ownership percentage in basis points (`41.10% -> 4110`).
    pub percentage_bps: i64,
    /// Snapshot as-of date.
    pub report_date: NaiveDate,
    /// SHA-256 hash of source release for deduplication.
    pub release_sha256: String,
}

/// Draft holding before entity resolution (without persisted row id and entity id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KseiHoldingDraft {
    /// Exchange ticker code, for example `BBCA`.
    pub ticker_code: String,
    /// Optional issuer name as provided by source row.
    pub issuer_name: Option<String>,
    /// Raw investor name exactly as imported.
    pub raw_investor_name: String,
    /// Optional KSEI investor type code.
    pub investor_type: Option<InvestorTypeCode>,
    /// Optional local/foreign classification.
    pub locality: Option<Locality>,
    /// Optional nationality text.
    pub nationality: Option<String>,
    /// Optional domicile text.
    pub domicile: Option<String>,
    /// Scripless holdings in shares.
    pub holdings_scripless: i64,
    /// Script holdings in shares.
    pub holdings_scrip: i64,
    /// Total shares held.
    pub total_shares: i64,
    /// Ownership percentage in basis points (`41.10% -> 4110`).
    pub percentage_bps: i64,
    /// Snapshot as-of date.
    pub report_date: NaiveDate,
}

/// Single Bing institutional ownership row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BingHolding {
    /// Internal holding row identifier.
    pub id: i64,
    /// Referenced ticker id.
    pub ticker_id: i64,
    /// Referenced entity id, unresolved when `None`.
    pub entity_id: Option<i64>,
    /// Raw investor name exactly as imported.
    pub raw_investor_name: String,
    /// Optional Bing investor type text.
    pub investor_type: Option<String>,
    /// Shares currently held.
    pub shares_held: Option<i64>,
    /// Share delta over period (`+buy`, `-sell`).
    pub shares_changed: Option<i64>,
    /// Ownership percentage in basis points.
    pub pct_ownership_bps: Option<i64>,
    /// Position value in USD.
    pub value_usd: Option<i64>,
    /// Source report date.
    pub report_date: NaiveDate,
    /// Institutional flow signal for the row.
    pub signal: FlowSignal,
    /// Import timestamp (unix epoch seconds).
    pub fetched_at: i64,
}

/// Raw row extracted from KSEI PDF before normalization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KseiRawRow {
    /// Date string as appears in PDF (for example `27-Feb-2026`).
    pub date: String,
    /// Share code string (for example `BBCA`).
    pub share_code: String,
    /// Issuer name as appears in source.
    pub issuer_name: String,
    /// Investor name as appears in source.
    pub investor_name: String,
    /// Investor type code string.
    pub investor_type: String,
    /// Local or foreign marker (`L` or `F`).
    pub local_foreign: String,
    /// Nationality text.
    pub nationality: String,
    /// Domicile text.
    pub domicile: String,
    /// Scripless holdings string in Indonesian locale format.
    pub holdings_scripless: String,
    /// Script holdings string in Indonesian locale format.
    pub holdings_scrip: String,
    /// Total holdings string in Indonesian locale format.
    pub total_holding_shares: String,
    /// Percentage string in Indonesian locale decimal format.
    pub percentage: String,
}

/// Raw holder object returned by Bing ownership endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BingHolderRaw {
    /// Investor name field from Bing payload.
    #[serde(alias = "investorName", alias = "InvestorName")]
    pub investor_name: Option<String>,
    /// Investor type field from Bing payload.
    #[serde(alias = "investorType", alias = "InvestorType")]
    pub investor_type: Option<String>,
    /// Shares held field from Bing payload.
    #[serde(alias = "sharesHeld", alias = "SharesHeld")]
    pub shares_held: Option<f64>,
    /// Shares changed field from Bing payload.
    #[serde(alias = "sharesChanged", alias = "SharesChanged")]
    pub shares_changed: Option<f64>,
    /// Percentage of shares outstanding field from Bing payload.
    #[serde(
        alias = "percentageOfSharesOutstanding",
        alias = "PercentageOfSharesOutstanding"
    )]
    pub pct_outstanding: Option<f64>,
    /// Position value field from Bing payload.
    #[serde(alias = "value", alias = "Value")]
    pub value: Option<f64>,
    /// Report date field from Bing payload.
    #[serde(alias = "reportDate", alias = "ReportDate")]
    pub report_date: Option<String>,
}

/// Combined ownership view for a ticker (KSEI and Bing merged).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerOwnership {
    /// Ticker metadata.
    pub ticker: Ticker,
    /// Latest KSEI as-of date.
    pub ksei_as_of: Option<NaiveDate>,
    /// Latest Bing reporting period label.
    pub bing_as_of: Option<String>,
    /// Combined holder rows for display.
    pub holders: Vec<HolderRow>,
    /// Concentration metrics calculated for the ticker.
    pub concentration: ConcentrationMetrics,
    /// Optional institutional flow breakdown.
    pub flow: Option<InstitutionalFlow>,
}

/// Single row in a combined holders table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolderRow {
    /// Ranking position in result set.
    pub rank: usize,
    /// Source dataset for the row.
    pub source: OwnershipSource,
    /// Canonical holder name or unresolved raw name.
    pub name: String,
    /// Optional referenced canonical entity id.
    pub entity_id: Option<i64>,
    /// Optional investor type label.
    pub investor_type: Option<String>,
    /// Optional local/foreign marker.
    pub locality: Option<Locality>,
    /// Held shares quantity.
    pub shares: i64,
    /// Ownership percentage in basis points.
    pub percentage_bps: i64,
    /// Optional flow signal (Bing rows only).
    pub signal: Option<FlowSignal>,
}

/// Concentration metrics for ownership distribution of a ticker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcentrationMetrics {
    /// Largest single holder percentage in basis points.
    pub top1_bps: i64,
    /// Sum of top 3 holder percentages in basis points.
    pub top3_bps: i64,
    /// Herfindahl-Hirschman index value.
    pub hhi: i64,
    /// Estimated free float in basis points (`10000 - known holders`).
    pub free_float_bps: i64,
    /// Number of KSEI holders above or equal to 1%.
    pub holder_count: usize,
}

/// Institutional flow summary from Bing for one ticker and period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstitutionalFlow {
    /// Reporting period label.
    pub period: String,
    /// Top buyer rows.
    pub top_buyers: Vec<HolderRow>,
    /// Top seller rows.
    pub top_sellers: Vec<HolderRow>,
    /// New position rows.
    pub new_positions: Vec<HolderRow>,
    /// Exited position rows.
    pub exited: Vec<HolderRow>,
}

/// Cross-holding summary for a single entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityHoldings {
    /// Canonical entity metadata.
    pub entity: Entity,
    /// Number of distinct tickers held.
    pub ticker_count: usize,
    /// Per-ticker ownership rows.
    pub holdings: Vec<EntityTickerRow>,
}

/// One ticker row within an entity holdings summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTickerRow {
    /// Ticker metadata.
    pub ticker: Ticker,
    /// Source dataset for the holding.
    pub source: OwnershipSource,
    /// Shares held quantity.
    pub shares: i64,
    /// Ownership percentage in basis points.
    pub percentage_bps: i64,
    /// Report as-of date.
    pub report_date: NaiveDate,
}

/// Cross-holder leaderboard row across multiple tickers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossHolderRow {
    /// Canonical entity metadata.
    pub entity: Entity,
    /// Number of distinct tickers held.
    pub ticker_count: usize,
    /// Summed ownership basis points across tickers.
    pub total_bps: i64,
}

/// Graph node used for ownership network visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Stable graph node id (`entity:42` or `ticker:BBCA`).
    pub id: String,
    /// Display label.
    pub label: String,
    /// Node category.
    pub node_type: GraphNodeType,
}

/// Graph edge used for ownership network visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    /// Edge weight in basis points.
    pub percentage_bps: i64,
    /// Source dataset of the edge.
    pub source: OwnershipSource,
}

/// Imported ownership release metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipRelease {
    /// Internal release id.
    pub id: i64,
    /// Optional source URL where release was obtained.
    pub source_url: Option<String>,
    /// SHA-256 hash for release-level deduplication.
    pub sha256: String,
    /// Release as-of date.
    pub as_of_date: NaiveDate,
    /// Parsed row count imported.
    pub row_count: usize,
    /// Import timestamp (unix epoch seconds).
    pub imported_at: i64,
}

/// Type of ownership change between two snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    /// Holder appears in `to` snapshot but not in `from`.
    New,
    /// Holder appears in `from` snapshot but not in `to`.
    Exited,
    /// Holder exists in both snapshots and percentage increased.
    Increased,
    /// Holder exists in both snapshots and percentage decreased.
    Decreased,
}

/// One ownership change row for a ticker-holder pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRow {
    /// Exchange ticker code.
    pub ticker_code: String,
    /// Canonical or raw holder name.
    pub entity_name: String,
    /// Change classification.
    pub change_type: ChangeType,
    /// Old percentage in basis points (from snapshot).
    pub old_bps: Option<i64>,
    /// New percentage in basis points (to snapshot).
    pub new_bps: Option<i64>,
    /// Delta in basis points (`new - old`, missing treated as 0).
    pub delta_bps: i64,
}

/// Row describing unresolved or low-confidence alias mappings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedRow {
    /// Raw investor name captured from source data.
    pub raw_name: String,
    /// Source marker (`ksei` or `bing`).
    pub source: String,
    /// Ticker code where the unresolved alias appears.
    pub ticker_code: String,
    /// Current canonical entity name when available.
    pub current_entity: Option<String>,
    /// Resolution confidence score.
    pub confidence: Option<f64>,
}
