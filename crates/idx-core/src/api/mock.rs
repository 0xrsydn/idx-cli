//! Fixture-backed provider for tests, enabled by the `testing` feature and
//! `IDX_USE_MOCK_PROVIDER`. Release builds do not contain it.

use super::stockbit::exodus;
use super::types::{Bar, Fundamentals, Interval, Intraday, OrderBook, Period, Quote};
use super::{
    BrokerSummaryProvider, FundamentalsProvider, HistoryProvider, IntradayProvider,
    OrderBookProvider, QuoteProvider, TradingSummaryProvider, idx, ipot, msn, stockbit, yahoo,
};
use crate::broker::types::{BrokerBoard, BrokerInvestor, BrokerQuery};
use crate::config::ProviderKind;
use crate::error::IdxError;

pub(crate) fn enabled() -> bool {
    std::env::var("IDX_USE_MOCK_PROVIDER").is_ok()
}

/// `IDX_MOCK_TODAY` (YYYY-MM-DD) pins today's date under the mock provider.
pub(crate) fn today() -> Option<chrono::NaiveDate> {
    if !enabled() {
        return None;
    }
    let raw = std::env::var("IDX_MOCK_TODAY").ok()?;
    chrono::NaiveDate::parse_from_str(&raw, "%Y-%m-%d").ok()
}

pub struct MockProvider {
    quote: Result<Quote, IdxError>,
    fundamentals: Result<Fundamentals, IdxError>,
    history: Result<Vec<Bar>, IdxError>,
}

impl MockProvider {
    pub fn from_fixtures(provider: ProviderKind) -> Self {
        Self::from_fixtures_with_history_verbose(provider, false)
    }

    pub fn from_fixtures_with_history_verbose(
        provider: ProviderKind,
        history_verbose: bool,
    ) -> Self {
        if std::env::var("IDX_MOCK_ERROR").is_ok() {
            return Self::with_error(IdxError::ProviderUnavailable);
        }

        match provider {
            ProviderKind::Yahoo => Self::from_yahoo_fixtures(history_verbose),
            ProviderKind::Msn => Self::from_msn_fixtures(),
            // `MockStockbitProvider` serves Stockbit; it has no history.
            ProviderKind::Stockbit => Self::with_error(IdxError::Unsupported(
                "Stockbit serves no price history".to_string(),
            )),
        }
    }

    fn from_yahoo_fixtures(history_verbose: bool) -> Self {
        let quote_raw = std::fs::read_to_string("tests/fixtures/chart_bbca_1d.json")
            .unwrap_or_else(|_| "{}".to_string());
        let history_path = std::env::var("IDX_MOCK_YAHOO_HISTORY_FIXTURE")
            .unwrap_or_else(|_| "tests/fixtures/chart_bbca_3mo.json".to_string());
        let history_raw =
            std::fs::read_to_string(&history_path).unwrap_or_else(|_| "{}".to_string());
        let fundamentals_raw = std::fs::read_to_string("tests/fixtures/quotesummary_bbca.json")
            .unwrap_or_else(|_| "{}".to_string());

        let quote = yahoo::parse_quote_from_str("BBCA.JK", &quote_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let fundamentals = yahoo::parse_fundamentals_from_str("BBCA.JK", &fundamentals_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let history =
            yahoo::parse_history_from_str_with_verbose("BBCA.JK", &history_raw, history_verbose)
                .map_err(|e| IdxError::ParseError(e.to_string()));

        Self {
            quote,
            fundamentals,
            history,
        }
    }

    fn from_msn_fixtures() -> Self {
        let quote_raw = std::fs::read_to_string("tests/fixtures/msn_quote_bbca.json")
            .unwrap_or_else(|_| "[]".to_string());
        let fundamentals_path = std::env::var("IDX_MOCK_MSN_KEYRATIOS_FIXTURE")
            .unwrap_or_else(|_| "tests/fixtures/msn_keyratios_bbca.json".to_string());
        let fundamentals_raw =
            std::fs::read_to_string(&fundamentals_path).unwrap_or_else(|_| "[]".to_string());

        let quote = msn::parse_quote_from_str("BBCA.JK", &quote_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let fundamentals = msn::parse_fundamentals_from_str(&fundamentals_raw, Some(&quote_raw))
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let history_raw = std::fs::read_to_string("tests/fixtures/msn_chart_bbca_3m.json")
            .unwrap_or_else(|_| "[]".to_string());
        let history = msn::parse_history_from_str("BBCA.JK", &history_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));

        Self {
            quote,
            fundamentals,
            history,
        }
    }

    pub fn with_error(err: IdxError) -> Self {
        Self {
            quote: Err(err.clone()),
            fundamentals: Err(err.clone()),
            history: Err(err),
        }
    }
}

impl QuoteProvider for MockProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let mut q = self.quote.clone()?;
        q.symbol = symbol.to_string();
        Ok(q)
    }
}

impl FundamentalsProvider for MockProvider {
    fn fundamentals(&self, _symbol: &str) -> Result<Fundamentals, IdxError> {
        self.fundamentals.clone()
    }
}

impl HistoryProvider for MockProvider {
    fn history(
        &self,
        _symbol: &str,
        _period: &Period,
        _interval: &Interval,
    ) -> Result<Vec<Bar>, IdxError> {
        self.history.clone()
    }
}

/// Stockbit from fixtures. Without a session (none stored or seeded through
/// `IDX_STOCKBIT_REFRESH_TOKEN`): the public page,
/// `tests/fixtures/stockbit_symbol_<code>.html`. With one: the session API,
/// `tests/fixtures/stockbit_exodus_orderbook_<code>.json` and
/// `stockbit_exodus_chart_candle_<code>_<window>.json`, with the market state
/// from `stockbit_exodus_market_session.json`. A code without a fixture is
/// Stockbit's 404. `IDX_MOCK_STOCKBIT_STATUS` (e.g. `403`) answers that HTTP
/// status instead, and `IDX_MOCK_ERROR` fails as for the other providers.
pub struct MockStockbitProvider;

impl MockStockbitProvider {
    fn failure(&self, symbol: &str) -> Option<IdxError> {
        if std::env::var("IDX_MOCK_ERROR").is_ok() {
            return Some(IdxError::ProviderUnavailable);
        }
        let status = std::env::var("IDX_MOCK_STOCKBIT_STATUS").ok()?;
        Some(stockbit::status_error(
            status.parse().unwrap_or(500),
            symbol,
        ))
    }

    fn snapshot(&self, symbol: &str) -> Result<Intraday, IdxError> {
        if let Some(err) = self.failure(symbol) {
            return Err(err);
        }
        let code = stockbit::stockbit_code(symbol)?;
        let path = format!(
            "tests/fixtures/stockbit_symbol_{}.html",
            code.to_ascii_lowercase()
        );
        match std::fs::read_to_string(path) {
            Ok(html) => stockbit::parse_symbol_page(symbol, &html),
            Err(_) => Err(stockbit::status_error(404, symbol)),
        }
    }

    /// A session API fixture, or the error a request would meet.
    fn exodus(&self, symbol: &str, name: &str) -> Result<String, IdxError> {
        if !stockbit::has_session() {
            return Err(IdxError::AuthError(stockbit::auth::NO_SESSION.to_string()));
        }
        if let Some(err) = self.failure(symbol) {
            return Err(err);
        }
        std::fs::read_to_string(format!("tests/fixtures/stockbit_exodus_{name}"))
            .map_err(|_| IdxError::SymbolNotFound(symbol.to_string()))
    }

    fn book(&self, symbol: &str) -> Result<OrderBook, IdxError> {
        let code = stockbit::stockbit_code(symbol)?.to_ascii_lowercase();
        let body = self.exodus(symbol, &format!("orderbook_{code}.json"))?;
        let state = self.exodus(symbol, "market_session.json")?;
        exodus::parse_orderbook(symbol, &body, exodus::parse_market_status(&state))
    }
}

impl QuoteProvider for MockStockbitProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        if stockbit::has_session() {
            return self.book(symbol).map(|book| exodus::quote_from_book(&book));
        }
        let mut quote = self.snapshot(symbol)?.quote;
        // Quotes are judged against the clock (behind the session, a week
        // old); a fixed trade time would make their output age.
        quote.as_of = None;
        Ok(quote)
    }
}

impl FundamentalsProvider for MockStockbitProvider {
    fn fundamentals(&self, symbol: &str) -> Result<Fundamentals, IdxError> {
        Err(stockbit::fundamentals_unsupported(symbol))
    }
}

impl IntradayProvider for MockStockbitProvider {
    fn intraday(&self, symbol: &str) -> Result<Intraday, IdxError> {
        self.snapshot(symbol)
    }
}

impl OrderBookProvider for MockStockbitProvider {
    fn orderbook(&self, symbol: &str) -> Result<OrderBook, IdxError> {
        self.book(symbol)
    }
}

impl HistoryProvider for MockStockbitProvider {
    fn history(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Bar>, IdxError> {
        let window = exodus::history_window(period, interval)
            .ok_or_else(|| exodus::unsupported_window(period, interval))?;
        let code = stockbit::stockbit_code(symbol)?.to_ascii_lowercase();
        let body = self.exodus(symbol, &format!("chart_candle_{code}_{window}.json"))?;
        exodus::daily_for_period(&body, period)
    }
}

/// Broker summaries from `tests/fixtures/ipot_brokersummary_<code>_<from>
/// [_<to>][_<investor>_<board>].html`; a query without a fixture reads as an
/// unknown code.
pub struct MockBrokerProvider;

impl BrokerSummaryProvider for MockBrokerProvider {
    fn broker_summary(
        &self,
        code: &str,
        query: &BrokerQuery,
    ) -> Result<ipot::BrokerSummaryRaw, IdxError> {
        if std::env::var("IDX_MOCK_ERROR").is_ok() {
            return Err(IdxError::ProviderUnavailable);
        }
        let mut name = format!(
            "ipot_brokersummary_{}_{}",
            code.to_ascii_lowercase(),
            query.from.format("%Y%m%d")
        );
        if query.to != query.from {
            name.push_str(&query.to.format("_%Y%m%d").to_string());
        }
        if (query.investor, query.board) != (BrokerInvestor::All, BrokerBoard::All) {
            name.push_str(&format!(
                "_{}_{}",
                query.investor.as_str(),
                query.board.as_str()
            ));
        }
        let html = std::fs::read_to_string(format!("tests/fixtures/{name}.html"))
            .unwrap_or_else(|_| "Invalid stock code!".to_string());
        ipot::summary_from_html(code, &html)
    }
}

/// Trading summaries from `tests/fixtures/idx_stocksummary_<date>.json`; a
/// date without a fixture had no trading.
pub struct MockTradingSummaryProvider;

impl TradingSummaryProvider for MockTradingSummaryProvider {
    fn stock_summary(
        &self,
        date: chrono::NaiveDate,
    ) -> Result<Vec<crate::flow::types::StockDay>, IdxError> {
        if std::env::var("IDX_MOCK_ERROR").is_ok() {
            return Err(IdxError::ProviderUnavailable);
        }
        let path = format!(
            "tests/fixtures/idx_stocksummary_{}.json",
            date.format("%Y%m%d")
        );
        match std::fs::read_to_string(path) {
            Ok(raw) => idx::parse_stock_summary(&raw, date),
            Err(_) => Ok(Vec::new()),
        }
    }
}
