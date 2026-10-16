//! Company financial data: profile, statements, earnings, sentiment,
//! insights, and news. These require the MSN provider.

use crate::api::types::{
    CompanyProfile, EarningsData, EarningsReport, FinancialStatements, InsightData, NewsItem,
    SentimentData,
};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;

pub struct Financial<'a> {
    pub(crate) idx: &'a Idx,
}

impl Financial<'_> {
    pub fn profile(
        &self,
        symbol: &str,
        policy: CachePolicy,
    ) -> Result<Fetched<CompanyProfile>, IdxError> {
        let resolved = self.idx.resolve(symbol)?;
        let provider = self.idx.provider.profile_provider(&resolved)?;
        self.fetch(
            "profile-v2",
            &resolved,
            &resolved,
            self.ttl(),
            policy,
            || provider.profile(&resolved),
        )
    }

    pub fn statements(
        &self,
        symbol: &str,
        filter: &StatementFilter,
        policy: CachePolicy,
    ) -> Result<Fetched<FinancialStatements>, IdxError> {
        let resolved = self.idx.resolve(symbol)?;
        let provider = self.idx.provider.financials_provider(&resolved)?;
        let fetched = self.fetch(
            "financials",
            &resolved,
            &resolved,
            self.ttl(),
            policy,
            || provider.financials(&resolved),
        )?;
        Ok(fetched.map(|mut financials| {
            if financials.instrument.symbol.trim().is_empty()
                || !financials.instrument.symbol.contains('.')
            {
                financials.instrument.symbol = resolved.clone();
            }
            filter.apply(&financials)
        }))
    }

    pub fn earnings(
        &self,
        symbol: &str,
        filter: EarningsFilter,
        policy: CachePolicy,
    ) -> Result<Fetched<EarningsReport>, IdxError> {
        let resolved = self.idx.resolve(symbol)?;
        let provider = self.idx.provider.earnings_provider(&resolved)?;
        let fetched = self.fetch("earnings", &resolved, &resolved, self.ttl(), policy, || {
            provider.earnings(&resolved)
        })?;
        Ok(fetched.map(|mut earnings| {
            if earnings.symbol.is_empty() {
                earnings.symbol = resolved.clone();
            }
            filter.apply(&earnings)
        }))
    }

    pub fn sentiment(
        &self,
        symbol: &str,
        policy: CachePolicy,
    ) -> Result<Fetched<SentimentData>, IdxError> {
        let resolved = self.idx.resolve(symbol)?;
        let provider = self.idx.provider.sentiment_provider(&resolved)?;
        self.fetch(
            "sentiment",
            &resolved,
            &resolved,
            self.ttl(),
            policy,
            || provider.sentiment(&resolved),
        )
    }

    pub fn insights(
        &self,
        symbol: &str,
        policy: CachePolicy,
    ) -> Result<Fetched<InsightData>, IdxError> {
        let resolved = self.idx.resolve(symbol)?;
        let provider = self.idx.provider.insights_provider(&resolved)?;
        let fetched = self.fetch(
            "insights-v2",
            &resolved,
            &resolved,
            self.ttl(),
            policy,
            || provider.insights(&resolved),
        )?;
        Ok(fetched.map(|mut insights| {
            if insights.symbol.is_empty() {
                insights.symbol = resolved.clone();
            }
            insights
        }))
    }

    pub fn news(
        &self,
        symbol: &str,
        limit: usize,
        policy: CachePolicy,
    ) -> Result<Fetched<Vec<NewsItem>>, IdxError> {
        let resolved = self.idx.resolve(symbol)?;
        // "ranked": entries cached before relevance ranking lack the field.
        let key = format!("{resolved}-{limit}-ranked");
        let provider = self.idx.provider.news_provider(&resolved)?;
        let ttl = self.idx.config.quote_ttl;
        let fetched = self.fetch("news", &key, &resolved, ttl, policy, || {
            provider.news(&resolved, limit)
        })?;
        Ok(fetched.map(|mut news| {
            for item in &mut news {
                if item.symbol.is_empty() {
                    item.symbol = resolved.clone();
                }
            }
            news
        }))
    }

    fn ttl(&self) -> u64 {
        self.idx.config.fundamental_ttl
    }

    fn fetch<T>(
        &self,
        bucket: &str,
        key: &str,
        subject: &str,
        ttl_secs: u64,
        policy: CachePolicy,
        fetch: impl FnOnce() -> Result<T, IdxError>,
    ) -> Result<Fetched<T>, IdxError>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let bucket = self.idx.bucket(bucket);
        cached_fetch(
            &self.idx.cache,
            CacheSpec {
                bucket: &bucket,
                key,
                ttl_secs,
                subject,
                source: self.idx.provider.kind().as_str(),
            },
            policy,
            fetch,
            Ok,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum StatementKind {
    Income,
    Balance,
    Cashflow,
}

/// Statements to keep; empty keeps all. Excluded sections become `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatementFilter {
    pub statements: Vec<StatementKind>,
}

impl StatementFilter {
    pub fn all() -> Self {
        Self::default()
    }

    fn includes(&self, kind: StatementKind) -> bool {
        self.statements.is_empty() || self.statements.contains(&kind)
    }

    pub fn apply(&self, financials: &FinancialStatements) -> FinancialStatements {
        FinancialStatements {
            instrument: financials.instrument.clone(),
            balance_sheet: self
                .includes(StatementKind::Balance)
                .then(|| financials.balance_sheet.clone())
                .flatten(),
            cash_flow: self
                .includes(StatementKind::Cashflow)
                .then(|| financials.cash_flow.clone())
                .flatten(),
            income_statement: self
                .includes(StatementKind::Income)
                .then(|| financials.income_statement.clone())
                .flatten(),
        }
    }
}

/// Earnings scope. With neither `forecast` nor `history` set both are kept;
/// with neither `annual` nor `quarterly` set every period is kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EarningsFilter {
    pub forecast: bool,
    pub history: bool,
    pub annual: bool,
    pub quarterly: bool,
}

impl EarningsFilter {
    fn include_forecast(self) -> bool {
        self.forecast || !self.history
    }

    fn include_history(self) -> bool {
        self.history || !self.forecast
    }

    fn includes_period(self, period_type: &str) -> bool {
        if !self.annual && !self.quarterly {
            return true;
        }

        match classify_earnings_period(period_type) {
            EarningsPeriodKind::Annual => self.annual || !self.quarterly,
            EarningsPeriodKind::Quarterly => self.quarterly || !self.annual,
            EarningsPeriodKind::Unknown => false,
        }
    }

    pub fn apply(self, report: &EarningsReport) -> EarningsReport {
        let filter_rows = |rows: &[EarningsData]| {
            rows.iter()
                .filter(|row| self.includes_period(&row.period_type))
                .cloned()
                .collect()
        };

        EarningsReport {
            symbol: report.symbol.clone(),
            eps_last_year: report.eps_last_year,
            revenue_last_year: report.revenue_last_year,
            forecast: if self.include_forecast() {
                filter_rows(&report.forecast)
            } else {
                Vec::new()
            },
            history: if self.include_history() {
                filter_rows(&report.history)
            } else {
                Vec::new()
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EarningsPeriodKind {
    Annual,
    Quarterly,
    Unknown,
}

fn classify_earnings_period(period_type: &str) -> EarningsPeriodKind {
    let trimmed = period_type.trim();
    if trimmed.is_empty() {
        return EarningsPeriodKind::Unknown;
    }

    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with('Q') {
        return EarningsPeriodKind::Quarterly;
    }
    if upper.starts_with("FY") || (upper.len() == 4 && upper.chars().all(|ch| ch.is_ascii_digit()))
    {
        return EarningsPeriodKind::Annual;
    }

    EarningsPeriodKind::Unknown
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        EarningsFilter, EarningsPeriodKind, StatementFilter, StatementKind,
        classify_earnings_period,
    };
    use crate::api::types::{
        EarningsData, EarningsReport, FinancialStatements, InstrumentInfo, StatementSection,
    };

    fn section(key: &str, value: f64) -> Option<StatementSection> {
        Some(StatementSection {
            values: BTreeMap::from([(key.into(), value)]),
            currency: "IDR".into(),
            report_date: "2026-01-01".into(),
            end_date: "2025-12-31".into(),
        })
    }

    #[test]
    fn filters_financial_statements_to_requested_sections() {
        let sample = FinancialStatements {
            instrument: InstrumentInfo {
                id: "123".into(),
                symbol: "BBCA.JK".into(),
                name: "BCA".into(),
            },
            balance_sheet: section("assets", 1.0),
            cash_flow: section("cash", 2.0),
            income_statement: section("income", 3.0),
        };

        let filtered = StatementFilter {
            statements: vec![StatementKind::Cashflow],
        }
        .apply(&sample);

        assert!(filtered.cash_flow.is_some());
        assert!(filtered.income_statement.is_none());
        assert!(filtered.balance_sheet.is_none());
        assert_eq!(filtered.instrument.symbol, "BBCA.JK");
    }

    fn earnings_row(period_type: &str, eps_forecast: Option<f64>) -> EarningsData {
        EarningsData {
            eps_actual: None,
            eps_forecast,
            eps_surprise: None,
            eps_surprise_pct: None,
            revenue_actual: None,
            revenue_forecast: None,
            revenue_surprise: None,
            earning_release_date: None,
            period_type: period_type.into(),
        }
    }

    #[test]
    fn filters_earnings_by_scope_and_period() {
        let report = EarningsReport {
            symbol: "BBCA.JK".into(),
            eps_last_year: 1_200.0,
            revenue_last_year: 100_000_000_000.0,
            forecast: vec![
                earnings_row("2026", Some(1_300.0)),
                earnings_row("Q12026", Some(330.0)),
            ],
            history: vec![earnings_row("2025", None)],
        };

        let filtered = EarningsFilter {
            forecast: true,
            history: false,
            annual: true,
            quarterly: false,
        }
        .apply(&report);

        assert_eq!(filtered.symbol, "BBCA.JK");
        assert!(filtered.history.is_empty());
        assert_eq!(filtered.forecast.len(), 1);
        assert_eq!(filtered.forecast[0].period_type, "2026");
    }

    #[test]
    fn classifies_earnings_periods() {
        assert_eq!(classify_earnings_period("2025"), EarningsPeriodKind::Annual);
        assert_eq!(
            classify_earnings_period("FY2026"),
            EarningsPeriodKind::Annual
        );
        assert_eq!(
            classify_earnings_period("Q12026"),
            EarningsPeriodKind::Quarterly
        );
        assert_eq!(classify_earnings_period(""), EarningsPeriodKind::Unknown);
    }
}
