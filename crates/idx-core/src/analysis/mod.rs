pub mod bandar;
pub mod fundamental;
pub mod report;
pub mod signals;
pub mod technical;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::api::history_provider;
use crate::api::types::{Fundamentals, Interval, Period};
use crate::broker::types::{BrokerBoard, BrokerInvestor, BrokerRange};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;
use crate::market::HistorySource;
use bandar::{BandarReport, analyze_bandar, check_top};
use fundamental::{
    FundamentalReport, GrowthReport, RiskReport, ValuationReport, analyze_fundamental,
    analyze_growth, analyze_risk, analyze_valuation,
};
use report::{TechnicalReport, build_technical_report};

/// Analysis reports. Reports are cached as computed, not their inputs.
pub struct Analysis<'a> {
    pub(crate) idx: &'a Idx,
}

/// Comparison results; symbols that failed are listed with their error.
pub struct Comparison {
    pub reports: Vec<Fetched<FundamentalReport>>,
    pub failures: Vec<(String, IdxError)>,
}

/// Readings weigh brokers against the value traded, which the source does
/// not total for one investor side.
fn check_investor(investor: BrokerInvestor) -> Result<(), IdxError> {
    if investor == BrokerInvestor::All {
        Ok(())
    } else {
        Err(IdxError::InvalidInput(format!(
            "bandar needs --investor all: the broker summary has no total for {} investors alone",
            investor.as_str()
        )))
    }
}

/// Bandar readings for several symbols, strongest accumulation first;
/// symbols that failed are listed with their error.
pub struct BandarScreen {
    pub reports: Vec<Fetched<BandarReport>>,
    pub failures: Vec<(String, IdxError)>,
}

/// The sessions and trades a bandar reading covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandarQuery {
    pub range: BrokerRange,
    pub investor: BrokerInvestor,
    pub board: BrokerBoard,
    /// Headline tier, one of `bandar::TIERS`.
    pub top: u8,
}

impl Analysis<'_> {
    /// Bandarmology over the broker summary `query` selects.
    pub fn bandar(
        &self,
        symbol: &str,
        query: &BandarQuery,
        policy: CachePolicy,
    ) -> Result<Fetched<BandarReport>, IdxError> {
        let top = check_top(query.top)?;
        check_investor(query.investor)?;
        let summary =
            self.idx
                .broker()
                .fetch(symbol, query.range, query.investor, query.board, policy)?;
        Ok(summary.map(|summary| analyze_bandar(&summary, top)))
    }

    /// [`Self::bandar`] for each symbol, ranked by the headline imbalance.
    /// Invalid input fails the whole screen; other errors fail one symbol.
    pub fn bandar_screen<S: AsRef<str>>(
        &self,
        symbols: &[S],
        query: &BandarQuery,
        policy: CachePolicy,
    ) -> Result<BandarScreen, IdxError> {
        check_top(query.top)?;
        check_investor(query.investor)?;
        let mut reports = Vec::new();
        let mut failures = Vec::new();
        for symbol in symbols {
            let resolved = self.idx.resolve(symbol.as_ref())?;
            match self.bandar(&resolved, query, policy) {
                Ok(report) => reports.push(report),
                Err(err @ IdxError::InvalidInput(_)) => return Err(err),
                Err(err) => failures.push((resolved, err)),
            }
        }
        if reports.is_empty() {
            return Err(failures
                .pop()
                .map(|(_, err)| err)
                .unwrap_or_else(|| IdxError::InvalidInput("no symbols to screen".to_string())));
        }
        // Strongest accumulation first; readings without an imbalance last.
        reports.sort_by_key(|report| std::cmp::Reverse(report.data.imbalance_bps));
        Ok(BandarScreen { reports, failures })
    }

    /// Technical analysis over one year of daily history from `source`
    /// (see `Market::history_source`).
    pub fn technical(
        &self,
        symbol: &str,
        source: HistorySource,
        policy: CachePolicy,
    ) -> Result<Fetched<TechnicalReport>, IdxError> {
        let idx = self.idx;
        let (provider_kind, provider) =
            history_provider(idx.provider.kind(), source.mode, idx.config.verbose)?;
        let bucket = format!("{}-technical", provider_kind.as_str());
        let resolved = idx.resolve(symbol)?;
        cached_fetch(
            &idx.cache,
            CacheSpec {
                bucket: &bucket,
                key: &resolved,
                ttl_secs: idx.config.quote_ttl,
                subject: &resolved,
                source: provider_kind.as_str(),
            },
            policy,
            || provider.history(&resolved, &Period::OneYear, &Interval::Day),
            |history| build_technical_report(&resolved, &history),
        )
    }

    pub fn growth(
        &self,
        symbol: &str,
        policy: CachePolicy,
    ) -> Result<Fetched<GrowthReport>, IdxError> {
        self.fundamental_report(symbol, "growth", policy, |_, f| analyze_growth(f))
    }

    pub fn valuation(
        &self,
        symbol: &str,
        policy: CachePolicy,
    ) -> Result<Fetched<ValuationReport>, IdxError> {
        self.fundamental_report(symbol, "valuation", policy, |_, f| analyze_valuation(f))
    }

    pub fn risk(&self, symbol: &str, policy: CachePolicy) -> Result<Fetched<RiskReport>, IdxError> {
        self.fundamental_report(symbol, "risk", policy, |_, f| analyze_risk(f))
    }

    pub fn fundamental(
        &self,
        symbol: &str,
        policy: CachePolicy,
    ) -> Result<Fetched<FundamentalReport>, IdxError> {
        self.fundamental_report(symbol, "fundamental", policy, analyze_fundamental)
    }

    /// Fundamental reports for each symbol. Fails only if every symbol fails
    /// (with the last error) or a symbol cannot be resolved.
    pub fn compare<S: AsRef<str>>(
        &self,
        symbols: &[S],
        policy: CachePolicy,
    ) -> Result<Comparison, IdxError> {
        let mut reports = Vec::new();
        let mut failures = Vec::new();
        for symbol in symbols {
            let resolved = self.idx.resolve(symbol.as_ref())?;
            match self.fundamental(&resolved, policy) {
                Ok(report) => reports.push(report),
                Err(err) => failures.push((resolved, err)),
            }
        }

        if reports.is_empty() {
            return Err(failures.pop().map(|(_, err)| err).unwrap_or_else(|| {
                IdxError::CacheMiss("fundamental/no symbols could be compared".to_string())
            }));
        }
        Ok(Comparison { reports, failures })
    }

    fn fundamental_report<T>(
        &self,
        symbol: &str,
        bucket: &str,
        policy: CachePolicy,
        analyzer: impl FnOnce(&str, &Fundamentals) -> T,
    ) -> Result<Fetched<T>, IdxError>
    where
        T: Serialize + DeserializeOwned,
    {
        let idx = self.idx;
        let resolved = idx.resolve(symbol)?;
        let bucket = idx.bucket(bucket);
        cached_fetch(
            &idx.cache,
            CacheSpec {
                bucket: &bucket,
                key: &resolved,
                ttl_secs: idx.config.fundamental_ttl,
                subject: &resolved,
                source: idx.provider.kind().as_str(),
            },
            policy,
            || idx.provider.market().fundamentals(&resolved),
            |fundamentals| Ok(analyzer(&resolved, &fundamentals)),
        )
    }
}
