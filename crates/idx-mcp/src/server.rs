//! MCP tools over idx-core. Each call runs on a blocking thread with a fresh
//! `Idx` (as one CLI invocation would), so tools share the CLI's on-disk
//! cache and ownership database.

use std::collections::BTreeMap;

use idx_core::analysis::BandarQuery;
use idx_core::analysis::bandar::{BandarReport, DEFAULT_TOP};
use idx_core::analysis::fundamental::FundamentalReport;
use idx_core::broker::types::BrokerRange;
use idx_core::config::CoreConfig;
use idx_core::error::IdxError;
use idx_core::financial::{EarningsFilter, StatementFilter};
use idx_core::flow::types::FlowRange;
use idx_core::investor::{HolderSource, Investor};
use idx_core::market::{ScreenFilter, ScreenQuery, ScreenRegion};
use idx_core::ownership::types::ChangeRow;
use idx_core::{CachePolicy, Fetched, Idx};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::sync::Semaphore;

use crate::params::{
    AnalysisKind, AnalyzeParams, BandarParams, BrokerParams, ChangesParams, CompanyParams,
    CompanySection, EntityParams, FlowParams, HistoryParams, ScreenParams, SearchParams,
    ShareholdersParams, SymbolsParams, SyncParams,
};

const SYNC_TOOL: &str = "ownership_sync";
const NEWS_LIMIT: usize = 10;
const SCREEN_LIMIT: usize = 50;
const SEARCH_LIMIT: usize = 20;
const CHANGES_LIMIT: usize = 200;
// Hard caps on what one call may request, so a misbehaving or prompt-injected
// agent cannot turn a call into unbounded upstream traffic or output.
const MAX_SYMBOLS: usize = 20;
const MAX_SCREEN_LIMIT: usize = 200;
const MAX_SEARCH_LIMIT: usize = 100;
// Pages above a few hundred rows (~40 KB) overflow some agents' tool output.
const MAX_CHANGES_LIMIT: usize = 300;
const CHANGES_TOP_TICKERS: usize = 10;

/// Tool calls running at once; the rest wait their turn.
static CALL_SLOTS: Semaphore = Semaphore::const_new(4);

const INSTRUCTIONS: &str = "Indonesian stock market (IDX) data. Market and financial \
tools fetch live data (cached on disk) and wrap it as {data, source, fetched_at, stale}; \
stale=true means an expired cache entry was served. Ownership tools read a local KSEI \
database of holders above 1%: call ownership_status first to see which release dates \
exist. Percentages ending in _bps are basis points (5494 = 54.94%). Errors carry a \
stable code, e.g. SYMBOLNOTFOUND, RATELIMITED, INVALIDINPUT.";

type ToolResult = Result<CallToolResult, ErrorData>;

#[derive(Clone)]
pub struct IdxServer {
    config: CoreConfig,
    allow_sync: bool,
    tool_router: ToolRouter<Self>,
}

impl IdxServer {
    pub fn new(config: CoreConfig, allow_sync: bool) -> Self {
        let mut tool_router = Self::tool_router();
        if !allow_sync {
            tool_router.remove_route(SYNC_TOOL);
        }
        Self {
            config,
            allow_sync,
            tool_router,
        }
    }

    /// Runs `f` against live data on a blocking thread.
    async fn live<T, F>(&self, f: F) -> ToolResult
    where
        T: Serialize + Send + 'static,
        F: FnOnce(&Idx) -> Result<T, IdxError> + Send + 'static,
    {
        let config = self.config.clone();
        blocking(move || f(&Idx::new(config)?)).await
    }

    /// Runs `f` against the ownership database, failing with a hint to sync
    /// when it is empty.
    async fn local<T, F>(&self, f: F) -> ToolResult
    where
        T: Serialize + Send + 'static,
        F: FnOnce(&Investor) -> Result<T, IdxError> + Send + 'static,
    {
        let investor = Investor::new(&self.config);
        let hint = if self.allow_sync {
            "call ownership_sync first"
        } else {
            "run `idx ownership sync`, or restart idx-mcp with --allow-sync"
        };
        blocking(move || {
            if !investor.status()?.populated {
                return Err(IdxError::NoMarketData(format!(
                    "ownership database is empty; {hint}"
                )));
            }
            f(&investor)
        })
        .await
    }
}

#[tool_router]
impl IdxServer {
    #[tool(
        description = "Latest quotes (price, change, volume, 52-week range, market cap) \
for one or more IDX tickers."
    )]
    async fn get_quotes(&self, params: Parameters<SymbolsParams>) -> ToolResult {
        let symbols = match checked_symbols(params.0.symbols) {
            Ok(symbols) => symbols,
            Err(err) => return err,
        };
        self.live(move |idx| {
            let quotes = idx.market().quotes(&symbols, CachePolicy::Default)?;
            Ok(json!({ "quotes": quotes }))
        })
        .await
    }

    #[tool(description = "OHLCV price history for one IDX ticker.")]
    async fn get_price_history(&self, params: Parameters<HistoryParams>) -> ToolResult {
        let HistoryParams {
            symbol,
            period,
            interval,
        } = params.0;
        self.live(move |idx| {
            let period = period.as_deref().unwrap_or("3mo").parse()?;
            let interval = interval.as_deref().unwrap_or("1d").parse()?;
            let source = idx.market().history_source(None);
            idx.market()
                .history(&symbol, &period, &interval, source, CachePolicy::Default)
        })
        .await
    }

    #[tool(
        description = "Run a stock screen (top/worst performers, high dividend, low P/E, \
52-week highs/lows, high volume, large cap)."
    )]
    async fn screen_stocks(&self, params: Parameters<ScreenParams>) -> ToolResult {
        let ScreenParams {
            filter,
            region,
            limit,
        } = params.0;
        self.live(move |idx| {
            let query = ScreenQuery {
                filter: filter
                    .as_deref()
                    .unwrap_or("top-performers")
                    .parse::<ScreenFilter>()?,
                region: region.as_deref().unwrap_or("id").parse::<ScreenRegion>()?,
                limit: bounded_limit("limit", limit, SCREEN_LIMIT, MAX_SCREEN_LIMIT)?,
            };
            idx.market().screen(&query, CachePolicy::Default)
        })
        .await
    }

    #[tool(
        description = "Broker summary for one IDX ticker: the top 10 buyers and top 10 sellers \
by lots over a session or date range, each with lots (1 lot = 100 shares), value and average \
price in IDR, plus totals. Buyers and sellers are ranked separately and gross, so a broker can \
appear on both sides. End of day only (IDX hides broker codes during trading); `kind` is the \
broker's own status (local, foreign, state_owned), not its clients'. `rounded` = true means \
figures of a million and more carry one decimal of precision."
    )]
    async fn get_broker_summary(&self, params: Parameters<BrokerParams>) -> ToolResult {
        let BrokerParams { symbol, scope } = params.0;
        self.live(move |idx| {
            let broker = idx.broker();
            let range = BrokerRange::from_args(
                scope.date.as_deref(),
                scope.from.as_deref(),
                scope.to.as_deref(),
                broker.today(),
            )?;
            broker.fetch(
                &symbol,
                range,
                scope.investor.as_deref().unwrap_or("all").parse()?,
                scope.board.as_deref().unwrap_or("all").parse()?,
                CachePolicy::Default,
            )
        })
        .await
    }

    #[tool(
        description = "Bandarmology for one or more IDX tickers from the broker summary: for \
the top 1, 3, 5 and 10 brokers per side, buy value minus sell value as basis points of value \
traded (`imbalance_bps`), with a `signal` from big_accumulation (>= +1500) through neutral \
(within +-500) to big_distribution (<= -1500), headlined by `top` (default 5). Reports come \
strongest accumulation first; tickers that fail are listed under `failures`. A heuristic over \
end-of-day broker codes: brokers trade for many clients, and `both_sides` lists brokers whose \
gross figures overstate their direction. `method` names the rules applied. `investor` must be \
`all`: the summary has no total for one investor side."
    )]
    async fn analyze_bandar(&self, params: Parameters<BandarParams>) -> ToolResult {
        let BandarParams {
            scope,
            symbols,
            top,
        } = params.0;
        let symbols = match checked_symbols(symbols) {
            Ok(symbols) => symbols,
            Err(err) => return err,
        };
        self.live(move |idx| {
            let query = BandarQuery {
                range: BrokerRange::from_args(
                    scope.date.as_deref(),
                    scope.from.as_deref(),
                    scope.to.as_deref(),
                    idx.broker().today(),
                )?,
                investor: scope.investor.as_deref().unwrap_or("all").parse()?,
                board: scope.board.as_deref().unwrap_or("all").parse()?,
                top: top.unwrap_or(DEFAULT_TOP),
            };
            let screen = idx
                .analysis()
                .bandar_screen(&symbols, &query, CachePolicy::Default)?;
            let reports: &[Fetched<BandarReport>] = &screen.reports;
            Ok(json!({ "reports": reports, "failures": failures_json(&screen.failures) }))
        })
        .await
    }

    #[tool(
        description = "Foreign investors' buying and selling of one IDX ticker, session by \
session (oldest first), from IDX's daily trading summary: foreign buy, sell and net volume in \
shares, `foreign_net_value` (net shares at the session's average price, an estimate, IDR), \
`change_bps`, and `non_regular_bps` (share of value traded outside the regular board). Totals \
cover the whole range; `foreign_share_bps` is foreign investors' share of trading. End of day."
    )]
    async fn get_foreign_flow(&self, params: Parameters<FlowParams>) -> ToolResult {
        let FlowParams {
            symbol,
            days,
            from,
            to,
        } = params.0;
        self.live(move |idx| {
            let flow = idx.flow();
            let range = FlowRange::from_args(days, from.as_deref(), to.as_deref(), flow.today())?;
            flow.foreign(&symbol, range, CachePolicy::Default)
        })
        .await
    }

    #[tool(
        description = "Company data for one IDX ticker: profile, financial statements, \
earnings, analyst sentiment, insights, and news. News items carry `relevance`: \
`company` (about this ticker) first, then `market` (general market news). Sections \
that fail are listed under `errors`; the call fails only if every section fails."
    )]
    async fn get_company(&self, params: Parameters<CompanyParams>) -> ToolResult {
        let CompanyParams { symbol, sections } = params.0;
        let mut sections = sections.unwrap_or_else(|| CompanySection::ALL.to_vec());
        dedup_in_order(&mut sections);
        if let Some(err) = empty_input("sections", &sections) {
            return err;
        }
        self.live(move |idx| {
            let financial = idx.financial();
            let policy = CachePolicy::Default;
            let mut report = SectionReport::new(&symbol);
            for section in sections {
                let name = section.as_str();
                match section {
                    CompanySection::Profile => report.add(name, financial.profile(&symbol, policy)),
                    CompanySection::Financials => report.add(
                        name,
                        financial.statements(&symbol, &StatementFilter::all(), policy),
                    ),
                    CompanySection::Earnings => report.add(
                        name,
                        financial.earnings(&symbol, EarningsFilter::default(), policy),
                    ),
                    CompanySection::Sentiment => {
                        report.add(name, financial.sentiment(&symbol, policy))
                    }
                    CompanySection::Insights => {
                        report.add(name, financial.insights(&symbol, policy))
                    }
                    CompanySection::News => {
                        report.add(name, financial.news(&symbol, NEWS_LIMIT, policy))
                    }
                }
            }
            report.finish()
        })
        .await
    }

    #[tool(
        description = "Analyze one IDX ticker: technical signals (trend, RSI, MACD, \
moving averages) and fundamental growth, valuation, and risk ratings. Analyses that fail \
are listed under `errors`; the call fails only if every analysis fails."
    )]
    async fn analyze_stock(&self, params: Parameters<AnalyzeParams>) -> ToolResult {
        let AnalyzeParams { symbol, kinds } = params.0;
        let mut kinds = kinds.unwrap_or_else(|| AnalysisKind::ALL.to_vec());
        dedup_in_order(&mut kinds);
        if let Some(err) = empty_input("kinds", &kinds) {
            return err;
        }
        self.live(move |idx| {
            let analysis = idx.analysis();
            let policy = CachePolicy::Default;
            let mut report = SectionReport::new(&symbol);
            for kind in kinds {
                let name = kind.as_str();
                match kind {
                    AnalysisKind::Technical => {
                        let source = idx.market().history_source(None);
                        report.add(name, analysis.technical(&symbol, source, policy))
                    }
                    AnalysisKind::Growth => report.add(name, analysis.growth(&symbol, policy)),
                    AnalysisKind::Valuation => {
                        report.add(name, analysis.valuation(&symbol, policy))
                    }
                    AnalysisKind::Risk => report.add(name, analysis.risk(&symbol, policy)),
                }
            }
            report.finish()
        })
        .await
    }

    #[tool(
        description = "Compare fundamental reports (growth, valuation, risk) across IDX \
tickers. Tickers that fail are listed under `failures`."
    )]
    async fn compare_stocks(&self, params: Parameters<SymbolsParams>) -> ToolResult {
        let symbols = match checked_symbols(params.0.symbols) {
            Ok(symbols) => symbols,
            Err(err) => return err,
        };
        self.live(move |idx| {
            let comparison = idx.analysis().compare(&symbols, CachePolicy::Default)?;
            let reports: &[Fetched<FundamentalReport>] = &comparison.reports;
            Ok(json!({ "reports": reports, "failures": failures_json(&comparison.failures) }))
        })
        .await
    }

    #[tool(
        description = "Shareholders above 1% of an IDX ticker from the latest KSEI release, \
with concentration metrics (top1, top3, HHI, free float)."
    )]
    async fn get_shareholders(&self, params: Parameters<ShareholdersParams>) -> ToolResult {
        let ShareholdersParams { ticker } = params.0;
        self.local(move |investor| investor.ticker(&ticker, HolderSource::All))
            .await
    }

    #[tool(description = "Every IDX holding (above 1%) of one investor, matched by name.")]
    async fn get_investor_holdings(&self, params: Parameters<EntityParams>) -> ToolResult {
        let EntityParams { entity } = params.0;
        self.local(move |investor| investor.entity(&entity)).await
    }

    #[tool(description = "Search investor names in the KSEI ownership database.")]
    async fn search_investors(&self, params: Parameters<SearchParams>) -> ToolResult {
        let SearchParams { query, limit } = params.0;
        self.local(move |investor| {
            let limit = bounded_limit("limit", limit, SEARCH_LIMIT, MAX_SEARCH_LIMIT)?;
            let entities = investor.search(&query, limit)?;
            Ok(json!({ "entities": entities }))
        })
        .await
    }

    #[tool(
        description = "Holder changes (new, exited, increased, decreased) between two KSEI \
release dates, optionally for one ticker. Rows come largest change first, one page at a time: \
`summary` covers all of them (counts by type, tickers with the most changes), and \
`next_offset` is the `offset` for the next page, or null after the last."
    )]
    async fn get_ownership_changes(&self, params: Parameters<ChangesParams>) -> ToolResult {
        let ChangesParams {
            from,
            to,
            ticker,
            limit,
            offset,
        } = params.0;
        self.local(move |investor| {
            let limit = bounded_limit("limit", limit, CHANGES_LIMIT, MAX_CHANGES_LIMIT).map_err(
                |error| match error {
                    IdxError::InvalidInput(message) => IdxError::InvalidInput(format!(
                        "{message}; page with `offset`, or use `summary` for an overview"
                    )),
                    other => other,
                },
            )?;
            let offset = offset.unwrap_or(0);
            let mut changes = investor.changes(&from, &to, ticker.as_deref())?;
            changes.sort_by(|a, b| {
                b.delta_bps
                    .abs()
                    .cmp(&a.delta_bps.abs())
                    .then_with(|| a.ticker_code.cmp(&b.ticker_code))
                    .then_with(|| a.entity_name.cmp(&b.entity_name))
            });
            let total = changes.len();
            let summary = changes_summary(&changes);
            let page: Vec<_> = changes.into_iter().skip(offset).take(limit).collect();
            let next_offset = (offset + page.len() < total).then_some(offset + page.len());
            Ok(json!({
                "from": from,
                "to": to,
                "total": total,
                "summary": summary,
                "offset": offset,
                "next_offset": next_offset,
                "changes": page,
            }))
        })
        .await
    }

    #[tool(
        description = "Ownership database status: whether it has data, and the imported \
KSEI release dates."
    )]
    async fn ownership_status(&self) -> ToolResult {
        let investor = Investor::new(&self.config);
        blocking(move || investor.status()).await
    }

    #[tool(
        description = "Download or refresh the KSEI ownership snapshot (several MB) into \
the local database."
    )]
    async fn ownership_sync(&self, params: Parameters<SyncParams>) -> ToolResult {
        let SyncParams { force } = params.0;
        let investor = Investor::new(&self.config);
        // idx-core's database lock keeps the swap away from open readers.
        blocking(move || investor.sync(None, force)).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for IdxServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("idx-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

/// Results keyed by section, with failed sections under `errors`.
struct SectionReport {
    symbol: String,
    sections: Map<String, Value>,
    errors: Map<String, Value>,
    last_error: Option<IdxError>,
}

impl SectionReport {
    fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            sections: Map::new(),
            errors: Map::new(),
            last_error: None,
        }
    }

    fn add<T: Serialize>(&mut self, name: &str, result: Result<T, IdxError>) {
        match result.and_then(to_json) {
            Ok(value) => {
                self.sections.insert(name.to_string(), value);
            }
            Err(err) => {
                self.errors
                    .insert(name.to_string(), Value::Object(error_body(&err)));
                self.last_error = Some(err);
            }
        }
    }

    fn finish(self) -> Result<Value, IdxError> {
        if self.sections.is_empty()
            && let Some(err) = self.last_error
        {
            return Err(err);
        }
        let mut out = Map::new();
        out.insert("symbol".into(), json!(self.symbol));
        out.extend(self.sections);
        if !self.errors.is_empty() {
            out.insert("errors".into(), Value::Object(self.errors));
        }
        Ok(Value::Object(out))
    }
}

async fn blocking<T, F>(f: F) -> ToolResult
where
    T: Serialize + Send + 'static,
    F: FnOnce() -> Result<T, IdxError> + Send + 'static,
{
    let _slot = CALL_SLOTS
        .acquire()
        .await
        .map_err(|err| ErrorData::internal_error(err.to_string(), None))?;
    let result = tokio::task::spawn_blocking(move || f().and_then(to_json))
        .await
        .map_err(|err| ErrorData::internal_error(err.to_string(), None))?;
    Ok(match result {
        Ok(value) => CallToolResult::structured(value),
        Err(err) => CallToolResult::structured_error(Value::Object(error_body(&err))),
    })
}

/// Per-symbol failures as error bodies carrying their symbol.
fn failures_json(failures: &[(String, IdxError)]) -> Vec<Value> {
    failures
        .iter()
        .map(|(symbol, err)| {
            let mut body = error_body(err);
            body.insert("symbol".into(), json!(symbol));
            Value::Object(body)
        })
        .collect()
}

fn input_error(message: String) -> ToolResult {
    let err = IdxError::InvalidInput(message);
    Ok(CallToolResult::structured_error(Value::Object(error_body(
        &err,
    ))))
}

/// An explicitly empty list is an input error, as in the CLI, rather than an
/// empty success an agent could mistake for data.
fn empty_input<T>(field: &str, values: &[T]) -> Option<ToolResult> {
    values
        .is_empty()
        .then(|| input_error(format!("{field} must not be empty")))
}

/// Non-empty, deduplicated (case-insensitively, first wins), and capped.
fn checked_symbols(symbols: Vec<String>) -> Result<Vec<String>, ToolResult> {
    if let Some(err) = empty_input("symbols", &symbols) {
        return Err(err);
    }
    let mut seen = std::collections::HashSet::new();
    let symbols: Vec<String> = symbols
        .into_iter()
        .filter(|symbol| seen.insert(symbol.trim().to_uppercase()))
        .collect();
    if symbols.len() > MAX_SYMBOLS {
        return Err(input_error(format!(
            "at most {MAX_SYMBOLS} symbols per call (got {})",
            symbols.len()
        )));
    }
    Ok(symbols)
}

/// Each section or analysis runs once, however often it is requested.
fn dedup_in_order<T: PartialEq + Copy>(values: &mut Vec<T>) {
    let mut seen = Vec::with_capacity(values.len());
    values.retain(|value| {
        let new = !seen.contains(value);
        if new {
            seen.push(*value);
        }
        new
    });
}

/// Counts by change type and the tickers with the most changes, so an agent
/// can answer an overview question without paging through every row.
fn changes_summary(changes: &[ChangeRow]) -> Value {
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_ticker: BTreeMap<&str, usize> = BTreeMap::new();
    for row in changes {
        let kind = serde_json::to_value(row.change_type)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        *by_type.entry(kind).or_default() += 1;
        *by_ticker.entry(row.ticker_code.as_str()).or_default() += 1;
    }
    let tickers = by_ticker.len();
    let mut top: Vec<_> = by_ticker.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    top.truncate(CHANGES_TOP_TICKERS);
    json!({
        "by_type": by_type,
        "tickers": tickers,
        "top_tickers": top
            .into_iter()
            .map(|(ticker, changes)| json!({ "ticker": ticker, "changes": changes }))
            .collect::<Vec<_>>(),
    })
}

fn bounded_limit(
    field: &str,
    value: Option<usize>,
    default: usize,
    max: usize,
) -> Result<usize, IdxError> {
    match value.unwrap_or(default) {
        0 => Err(IdxError::InvalidInput(format!(
            "{field} must be at least 1"
        ))),
        n if n > max => Err(IdxError::InvalidInput(format!(
            "{field} must be at most {max} (got {n})"
        ))),
        n => Ok(n),
    }
}

fn to_json<T: Serialize>(value: T) -> Result<Value, IdxError> {
    serde_json::to_value(value).map_err(|err| IdxError::ParseError(err.to_string()))
}

/// The CLI's JSON error shape: `{error, code, message}`.
fn error_body(err: &IdxError) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("error".into(), json!(true));
    body.insert("code".into(), json!(err.code().to_string()));
    body.insert("message".into(), json!(err.to_string()));
    body
}
