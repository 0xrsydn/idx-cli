//! Fixture-backed provider for tests, enabled by the `testing` feature and
//! `IDX_USE_MOCK_PROVIDER`. Release builds do not contain it.

use super::types::{Bar, Fundamentals, Interval, Period, Quote};
use super::{FundamentalsProvider, HistoryProvider, QuoteProvider, msn, yahoo};
use crate::config::ProviderKind;
use crate::error::IdxError;

pub(crate) fn enabled() -> bool {
    std::env::var("IDX_USE_MOCK_PROVIDER").is_ok()
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
