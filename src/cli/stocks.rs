use std::cmp::Ordering;

use clap::{Args, Subcommand, ValueEnum};
use serde::{Serialize, de::DeserializeOwned};

use crate::analysis::fundamental::{
    FundamentalReport, GrowthReport, RiskReport, ValuationReport, analyze_fundamental,
    analyze_growth, analyze_risk, analyze_valuation,
};
use crate::analysis::signals::{self, Signal, TechnicalSignal};
use crate::analysis::technical;
use crate::api::types::{
    CompanyProfile, EarningsReport, FinancialStatements, Fundamentals, InsightData, Interval,
    NewsItem, Ohlc, Period, Quote, SentimentData,
};
use crate::api::{MarketDataProvider, SelectedProvider, history_provider};
use crate::cache::Cache;
use crate::config::{HistoryProviderKind, IdxConfig, ProviderKind};
use crate::error::IdxError;
use crate::output::{
    MacdSnapshot, TechnicalReport, VolumeSnapshot, render_compare, render_earnings,
    render_financials, render_fundamental, render_growth, render_history, render_insights,
    render_news, render_profile, render_quotes, render_risk, render_screener, render_sentiment,
    render_technical, render_valuation,
};
use crate::runtime;

struct FundamentalCacheSpec {
    bucket: String,
    ttl_secs: u64,
}

struct CacheFetchSpec<'a> {
    bucket: &'a str,
    key: &'a str,
    ttl_secs: u64,
    subject: &'a str,
}

const SCREENER_FILTERS: &[&str] = &[
    "top-performers",
    "worst-performers",
    "high-dividend",
    "low-pe",
    "52w-high",
    "52w-low",
    "high-volume",
    "large-cap",
];

const SCREENER_REGIONS: &[&str] = &["id", "us", "sg", "hk", "jp"];

fn cache_bucket(provider: ProviderKind, key: &str) -> String {
    format!("{}-{key}", provider.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FinancialStatementKind {
    Income,
    Balance,
    Cashflow,
}

#[derive(Debug, Clone, Args, Default)]
pub struct FinancialsFilterArgs {
    /// Limit output to one or more statements.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    statement: Vec<FinancialStatementKind>,
}

#[derive(Debug, Clone, Copy, Args, Default)]
pub struct EarningsFilterArgs {
    /// Only include forward earnings rows.
    #[arg(long)]
    forecast: bool,
    /// Only include historical earnings rows.
    #[arg(long)]
    history: bool,
    /// Only include annual periods.
    #[arg(long)]
    annual: bool,
    /// Only include quarterly periods.
    #[arg(long)]
    quarterly: bool,
}

impl EarningsFilterArgs {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EarningsPeriodKind {
    Annual,
    Quarterly,
    Unknown,
}

#[derive(Debug, Args)]
#[command(about = "Stock data and analysis")]
pub struct StocksCmd {
    #[command(subcommand)]
    pub command: StocksSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum StocksSubcommand {
    #[command(
        about = "Get real-time stock quotes",
        after_help = "Examples:\n  idx stocks quote BBCA\n  idx stocks quote BBCA,BBRI,BMRI\n  idx -o json stocks quote BBCA"
    )]
    Quote {
        /// One or more symbols, comma-separated or space-separated.
        symbols: Vec<String>,
    },
    #[command(
        about = "Get historical OHLC data",
        after_help = "Examples:\n  idx stocks history BBCA --period 3mo\n  idx stocks history BBCA --period 1y --interval 1wk"
    )]
    History {
        /// Single ticker symbol (e.g. BBCA).
        symbol: String,
        #[arg(long, value_enum, default_value_t = Period::ThreeMonths)]
        period: Period,
        #[arg(long, value_enum, default_value_t = Interval::Day)]
        interval: Interval,
        #[arg(long, value_enum)]
        history_provider: Option<HistoryProviderKind>,
    },
    #[command(
        about = "Run technical analysis on a stock",
        after_help = "Examples:\n  idx stocks technical BBCA\n  idx -o json stocks technical BBCA"
    )]
    Technical {
        /// Single ticker symbol (e.g. BBCA).
        symbol: String,
        #[arg(long, value_enum)]
        history_provider: Option<HistoryProviderKind>,
    },
    #[command(
        about = "Run growth analysis on a stock",
        after_help = "Examples:\n  idx stocks growth BBCA\n  idx -o json stocks growth BBCA"
    )]
    Growth {
        /// Single ticker symbol (e.g. BBCA).
        symbol: String,
    },
    #[command(
        about = "Run valuation analysis on a stock",
        after_help = "Examples:\n  idx stocks valuation BBCA\n  idx -o json stocks valuation BBCA"
    )]
    Valuation {
        /// Single ticker symbol (e.g. BBCA).
        symbol: String,
    },
    #[command(
        about = "Run risk analysis on a stock",
        after_help = "Examples:\n  idx stocks risk BBCA\n  idx -o json stocks risk BBCA"
    )]
    Risk {
        /// Single ticker symbol (e.g. BBCA).
        symbol: String,
    },
    #[command(
        about = "Run full fundamental analysis on a stock",
        after_help = "Examples:\n  idx stocks fundamental BBCA\n  idx -o json stocks fundamental BBCA"
    )]
    Fundamental {
        /// Single ticker symbol (e.g. BBCA).
        symbol: String,
    },
    #[command(about = "Get company profile")]
    Profile { symbol: String },
    #[command(
        about = "Get financial statements",
        after_help = "Examples:\n  idx stocks financials BBCA\n  idx stocks financials BBCA --statement income\n  idx stocks financials BBCA --statement income,balance"
    )]
    Financials {
        symbol: String,
        #[command(flatten)]
        filters: FinancialsFilterArgs,
    },
    #[command(
        about = "Get earnings report",
        after_help = "Examples:\n  idx stocks earnings BBCA\n  idx stocks earnings BBCA --history --quarterly\n  idx -o json stocks earnings BBCA --forecast --annual"
    )]
    Earnings {
        symbol: String,
        #[command(flatten)]
        filters: EarningsFilterArgs,
    },
    #[command(about = "Get crowd sentiment")]
    Sentiment { symbol: String },
    #[command(about = "Get AI insights")]
    Insights { symbol: String },
    #[command(about = "Get stock news")]
    News {
        symbol: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    #[command(about = "MSN screener")]
    Screen {
        #[arg(long, default_value = "top-performers")]
        filter: String,
        #[arg(long, default_value = "id")]
        region: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    #[command(
        about = "Compare fundamentals across stocks",
        after_help = "Examples:\n  idx stocks compare BBCA BBRI BMRI\n  idx stocks compare BBCA,BBRI,BMRI\n  idx -o json stocks compare BBCA,BBRI"
    )]
    Compare {
        /// One or more symbols, comma-separated or space-separated.
        symbols: Vec<String>,
    },
}

pub fn handle(
    cmd: &StocksCmd,
    config: &IdxConfig,
    provider: &SelectedProvider,
    offline: bool,
    no_cache: bool,
    verbose: bool,
) -> Result<(), IdxError> {
    if offline && no_cache {
        return Err(IdxError::InvalidInput(
            "cannot combine --offline with --no-cache".to_string(),
        ));
    }

    let cache = Cache::new()?;

    match &cmd.command {
        StocksSubcommand::Quote { symbols } => {
            let quote_bucket = cache_bucket(provider.kind(), "quote");
            let mut quotes = Vec::new();
            for sym in symbols.iter().flat_map(|s| s.split(',')) {
                let resolved = crate::api::resolve_symbol(sym, &config.exchange)?;
                if !no_cache && let Some(q) = cache.get(&quote_bucket, &resolved)? {
                    quotes.push(q);
                    continue;
                }
                if offline {
                    let stale = cache
                        .get_stale(&quote_bucket, &resolved)?
                        .ok_or_else(|| IdxError::CacheMiss(format!("{quote_bucket}/{resolved}")))?;
                    quotes.push(stale);
                    continue;
                }

                match provider.market().quote(&resolved) {
                    Ok(q) => {
                        if !no_cache {
                            cache.put(&quote_bucket, &resolved, &q, config.quote_ttl)?;
                        }
                        quotes.push(q);
                    }
                    Err(err) => {
                        if !no_cache
                            && let Some(stale) = cache.get_stale(&quote_bucket, &resolved)?
                        {
                            runtime::warn(format!(
                                "network failed, serving stale cache for {resolved}"
                            ));
                            quotes.push(stale);
                            continue;
                        }
                        return Err(err);
                    }
                }
            }
            render_quotes(&quotes, &config.output, config.no_color)
        }
        StocksSubcommand::History {
            symbol,
            period,
            interval,
            history_provider: history_provider_override,
        } => {
            let history_mode = history_provider_override.unwrap_or(config.history_provider);
            let (history_source, hist_provider) =
                history_provider(provider.kind(), history_mode, verbose)?;
            if matches!(history_mode, HistoryProviderKind::Auto)
                && history_source != provider.kind()
                && !matches!(config.output, crate::output::OutputFormat::Json)
            {
                runtime::info(format!(
                    "history provider fallback active ({} -> {})",
                    provider.kind().as_str(),
                    history_source.as_str()
                ));
            }
            let history_bucket = format!("{}-history", history_source.as_str());
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let key = format!("{}-{}", period.as_str(), interval.as_str());
            if !no_cache
                && let Some(history) = cache.get::<Vec<crate::api::types::Ohlc>>(
                    &history_bucket,
                    &format!("{resolved}-{key}"),
                )?
            {
                return render_history(&resolved, &history, &config.output);
            }
            if offline {
                let stale = cache
                    .get_stale::<Vec<crate::api::types::Ohlc>>(
                        &history_bucket,
                        &format!("{resolved}-{key}"),
                    )?
                    .ok_or_else(|| {
                        IdxError::CacheMiss(format!("{history_bucket}/{resolved}-{key}"))
                    })?;
                return render_history(&resolved, &stale, &config.output);
            }

            match hist_provider.history(&resolved, period, interval) {
                Ok(history) => {
                    if !no_cache {
                        cache.put(
                            &history_bucket,
                            &format!("{resolved}-{key}"),
                            &history,
                            config.quote_ttl,
                        )?;
                    }
                    render_history(&resolved, &history, &config.output)
                }
                Err(err) => {
                    if !no_cache
                        && let Some(stale) = cache.get_stale::<Vec<crate::api::types::Ohlc>>(
                            &history_bucket,
                            &format!("{resolved}-{key}"),
                        )?
                    {
                        runtime::warn(format!(
                            "network failed, serving stale cache for {resolved}"
                        ));
                        return render_history(&resolved, &stale, &config.output);
                    }
                    Err(err)
                }
            }
        }
        StocksSubcommand::Technical {
            symbol,
            history_provider: history_provider_override,
        } => {
            let history_mode = history_provider_override.unwrap_or(config.history_provider);
            let (history_source, hist_provider) =
                history_provider(provider.kind(), history_mode, verbose)?;
            if matches!(history_mode, HistoryProviderKind::Auto)
                && history_source != provider.kind()
                && !matches!(config.output, crate::output::OutputFormat::Json)
            {
                runtime::info(format!(
                    "history provider fallback active ({} -> {})",
                    provider.kind().as_str(),
                    history_source.as_str()
                ));
            }
            let technical_bucket = format!("{}-technical", history_source.as_str());
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            if !no_cache
                && let Some(report) = cache.get::<TechnicalReport>(&technical_bucket, &resolved)?
            {
                return render_technical(&report, &config.output, config.no_color);
            }
            if offline {
                let stale = cache
                    .get_stale::<TechnicalReport>(&technical_bucket, &resolved)?
                    .ok_or_else(|| IdxError::CacheMiss(format!("{technical_bucket}/{resolved}")))?;
                return render_technical(&stale, &config.output, config.no_color);
            }

            match hist_provider.history(&resolved, &Period::OneYear, &Interval::Day) {
                Ok(history) => {
                    let report = build_technical_report(&resolved, &history)?;
                    if !no_cache {
                        cache.put(&technical_bucket, &resolved, &report, config.quote_ttl)?;
                    }
                    render_technical(&report, &config.output, config.no_color)
                }
                Err(err) => {
                    if !no_cache
                        && let Some(stale) =
                            cache.get_stale::<TechnicalReport>(&technical_bucket, &resolved)?
                    {
                        runtime::warn(format!(
                            "network failed, serving stale cache for {resolved}"
                        ));
                        return render_technical(&stale, &config.output, config.no_color);
                    }
                    Err(err)
                }
            }
        }
        StocksSubcommand::Growth { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let report: GrowthReport = fetch_fundamental_analysis_report(
                &cache,
                provider.market(),
                &resolved,
                FundamentalCacheSpec {
                    bucket: cache_bucket(provider.kind(), "growth"),
                    ttl_secs: config.fundamental_ttl,
                },
                offline,
                no_cache,
                |_, fundamentals| analyze_growth(fundamentals),
            )?;
            render_growth(&resolved, &report, &config.output, config.no_color)
        }
        StocksSubcommand::Valuation { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let report: ValuationReport = fetch_fundamental_analysis_report(
                &cache,
                provider.market(),
                &resolved,
                FundamentalCacheSpec {
                    bucket: cache_bucket(provider.kind(), "valuation"),
                    ttl_secs: config.fundamental_ttl,
                },
                offline,
                no_cache,
                |_, fundamentals| analyze_valuation(fundamentals),
            )?;
            render_valuation(&resolved, &report, &config.output, config.no_color)
        }
        StocksSubcommand::Risk { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let report: RiskReport = fetch_fundamental_analysis_report(
                &cache,
                provider.market(),
                &resolved,
                FundamentalCacheSpec {
                    bucket: cache_bucket(provider.kind(), "risk"),
                    ttl_secs: config.fundamental_ttl,
                },
                offline,
                no_cache,
                |_, fundamentals| analyze_risk(fundamentals),
            )?;
            render_risk(&resolved, &report, &config.output, config.no_color)
        }
        StocksSubcommand::Fundamental { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let report: FundamentalReport = fetch_fundamental_analysis_report(
                &cache,
                provider.market(),
                &resolved,
                FundamentalCacheSpec {
                    bucket: cache_bucket(provider.kind(), "fundamental"),
                    ttl_secs: config.fundamental_ttl,
                },
                offline,
                no_cache,
                analyze_fundamental,
            )?;
            render_fundamental(&report, &config.output, config.no_color)
        }
        StocksSubcommand::Profile { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let bucket = cache_bucket(provider.kind(), "profile-v2");
            let profile_provider = provider.profile_provider(&resolved)?;
            let profile: CompanyProfile = fetch_with_cache(
                &cache,
                CacheFetchSpec {
                    bucket: &bucket,
                    key: &resolved,
                    ttl_secs: config.fundamental_ttl,
                    subject: &resolved,
                },
                offline,
                no_cache,
                || profile_provider.profile(&resolved),
            )?;
            render_profile(&profile, &config.output)
        }
        StocksSubcommand::Financials { symbol, filters } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let bucket = cache_bucket(provider.kind(), "financials");
            let financials_provider = provider.financials_provider(&resolved)?;
            let mut financials: FinancialStatements = fetch_with_cache(
                &cache,
                CacheFetchSpec {
                    bucket: &bucket,
                    key: &resolved,
                    ttl_secs: config.fundamental_ttl,
                    subject: &resolved,
                },
                offline,
                no_cache,
                || financials_provider.financials(&resolved),
            )?;
            if financials.instrument.symbol.trim().is_empty()
                || !financials.instrument.symbol.contains('.')
            {
                financials.instrument.symbol = resolved.clone();
            }
            let filtered = filter_financial_statements(&financials, filters);
            render_financials(&filtered, &config.output)
        }
        StocksSubcommand::Earnings { symbol, filters } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let bucket = cache_bucket(provider.kind(), "earnings");
            let earnings_provider = provider.earnings_provider(&resolved)?;
            let mut earnings: EarningsReport = fetch_with_cache(
                &cache,
                CacheFetchSpec {
                    bucket: &bucket,
                    key: &resolved,
                    ttl_secs: config.fundamental_ttl,
                    subject: &resolved,
                },
                offline,
                no_cache,
                || earnings_provider.earnings(&resolved),
            )?;
            if earnings.symbol.is_empty() {
                earnings.symbol = resolved.clone();
            }
            let filtered = filter_earnings_report(&earnings, filters);
            render_earnings(&filtered, &config.output)
        }
        StocksSubcommand::Sentiment { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let bucket = cache_bucket(provider.kind(), "sentiment");
            let sentiment_provider = provider.sentiment_provider(&resolved)?;
            let sentiment: SentimentData = fetch_with_cache(
                &cache,
                CacheFetchSpec {
                    bucket: &bucket,
                    key: &resolved,
                    ttl_secs: config.fundamental_ttl,
                    subject: &resolved,
                },
                offline,
                no_cache,
                || sentiment_provider.sentiment(&resolved),
            )?;
            render_sentiment(&sentiment, &config.output)
        }
        StocksSubcommand::Insights { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let bucket = cache_bucket(provider.kind(), "insights-v2");
            let insights_provider = provider.insights_provider(&resolved)?;
            let mut insights: InsightData = fetch_with_cache(
                &cache,
                CacheFetchSpec {
                    bucket: &bucket,
                    key: &resolved,
                    ttl_secs: config.fundamental_ttl,
                    subject: &resolved,
                },
                offline,
                no_cache,
                || insights_provider.insights(&resolved),
            )?;
            if insights.symbol.is_empty() {
                insights.symbol = resolved.clone();
            }
            render_insights(&insights, &config.output)
        }
        StocksSubcommand::News { symbol, limit } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange)?;
            let bucket = cache_bucket(provider.kind(), "news");
            let key = format!("{resolved}-{limit}");
            let news_provider = provider.news_provider(&resolved)?;
            let mut news: Vec<NewsItem> = fetch_with_cache(
                &cache,
                CacheFetchSpec {
                    bucket: &bucket,
                    key: &key,
                    ttl_secs: config.quote_ttl,
                    subject: &resolved,
                },
                offline,
                no_cache,
                || news_provider.news(&resolved, *limit),
            )?;
            for item in &mut news {
                if item.symbol.is_empty() {
                    item.symbol = resolved.clone();
                }
            }
            render_news(&news, &config.output)
        }
        StocksSubcommand::Screen {
            filter,
            region,
            limit,
        } => {
            let filter_key = screener_filter_key(filter)?;
            let region_key = screener_region_key(region)?;
            // For filters that fall back to topperfs, fetch all stocks so
            // client-side sorting picks the correct top N.
            let needs_full_fetch = matches!(filter.as_str(), "high-volume" | "large-cap");
            let fetch_limit = if needs_full_fetch { 500 } else { *limit };
            let bucket = cache_bucket(provider.kind(), "screen");
            let key = format!("{filter}:{region}:{fetch_limit}");
            let screener_provider = provider.screener_provider("screen")?;
            let mut quotes: Vec<Quote> = fetch_with_cache(
                &cache,
                CacheFetchSpec {
                    bucket: &bucket,
                    key: &key,
                    ttl_secs: config.quote_ttl,
                    subject: "screen",
                },
                offline,
                no_cache,
                || screener_provider.screener(filter_key, region_key, fetch_limit),
            )?;
            sort_screener_quotes(&mut quotes, filter);
            quotes.truncate(*limit);
            render_screener(&quotes, &config.output, config.no_color)
        }
        StocksSubcommand::Compare { symbols } => {
            let mut reports: Vec<FundamentalReport> = Vec::new();
            let mut last_error = None;

            for sym in symbols.iter().flat_map(|s| s.split(',')) {
                let resolved = crate::api::resolve_symbol(sym, &config.exchange)?;
                match fetch_fundamental_analysis_report(
                    &cache,
                    provider.market(),
                    &resolved,
                    FundamentalCacheSpec {
                        bucket: cache_bucket(provider.kind(), "fundamental"),
                        ttl_secs: config.fundamental_ttl,
                    },
                    offline,
                    no_cache,
                    analyze_fundamental,
                ) {
                    Ok(report) => reports.push(report),
                    Err(err) => {
                        runtime::warn(format!(
                            "failed to fetch fundamentals for {resolved}: {err}"
                        ));
                        last_error = Some(err);
                    }
                }
            }

            if reports.is_empty() {
                return Err(last_error.unwrap_or_else(|| {
                    IdxError::CacheMiss("fundamental/no symbols could be compared".to_string())
                }));
            }

            render_compare(&reports, &config.output, config.no_color)
        }
    }
}

fn fetch_with_cache<T, F>(
    cache: &Cache,
    cache_spec: CacheFetchSpec<'_>,
    offline: bool,
    no_cache: bool,
    fetch_fn: F,
) -> Result<T, IdxError>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce() -> Result<T, IdxError>,
{
    if !offline
        && !no_cache
        && let Some(cached) = cache.get::<T>(cache_spec.bucket, cache_spec.key)?
    {
        return Ok(cached);
    }

    if offline {
        if no_cache {
            return Err(IdxError::InvalidInput(
                "cannot combine --offline with --no-cache".to_string(),
            ));
        }

        return cache
            .get_stale::<T>(cache_spec.bucket, cache_spec.key)?
            .ok_or_else(|| {
                IdxError::CacheMiss(format!("{}/{}", cache_spec.bucket, cache_spec.key))
            });
    }

    match fetch_fn() {
        Ok(data) => {
            if !no_cache {
                cache.put(
                    cache_spec.bucket,
                    cache_spec.key,
                    &data,
                    cache_spec.ttl_secs,
                )?;
            }
            Ok(data)
        }
        Err(err) => {
            if !no_cache
                && let Some(stale) = cache.get_stale::<T>(cache_spec.bucket, cache_spec.key)?
            {
                runtime::warn(format!(
                    "network failed, serving stale cache for {}",
                    cache_spec.subject
                ));
                return Ok(stale);
            }
            Err(err)
        }
    }
}

fn fetch_fundamental_analysis_report<T, F>(
    cache: &Cache,
    provider: &dyn MarketDataProvider,
    resolved: &str,
    cache_spec: FundamentalCacheSpec,
    offline: bool,
    no_cache: bool,
    analyzer: F,
) -> Result<T, IdxError>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce(&str, &Fundamentals) -> T,
{
    if !no_cache && let Some(report) = cache.get::<T>(&cache_spec.bucket, resolved)? {
        return Ok(report);
    }

    if offline {
        return cache
            .get_stale::<T>(&cache_spec.bucket, resolved)?
            .ok_or_else(|| IdxError::CacheMiss(format!("{}/{resolved}", cache_spec.bucket)));
    }

    match provider.fundamentals(resolved) {
        Ok(fundamentals) => {
            let report = analyzer(resolved, &fundamentals);
            if !no_cache {
                cache.put(&cache_spec.bucket, resolved, &report, cache_spec.ttl_secs)?;
            }
            Ok(report)
        }
        Err(err) => {
            if !no_cache && let Some(stale) = cache.get_stale::<T>(&cache_spec.bucket, resolved)? {
                runtime::warn(format!(
                    "network failed, serving stale cache for {resolved}"
                ));
                return Ok(stale);
            }
            Err(err)
        }
    }
}

fn build_technical_report(symbol: &str, history: &[Ohlc]) -> Result<TechnicalReport, IdxError> {
    let latest = history
        .last()
        .ok_or_else(|| IdxError::ParseError(format!("no history available for {symbol}")))?;
    let closes: Vec<f64> = history.iter().map(|item| item.close as f64).collect();
    let volumes: Vec<f64> = history.iter().map(|item| item.volume as f64).collect();

    let sma20 = last_value(&technical::sma(&closes, 20));
    let sma50 = last_value(&technical::sma(&closes, 50));
    let sma200 = last_value(&technical::sma(&closes, 200));
    let rsi14 = last_value(&technical::rsi(&closes, 14));
    let macd = technical::macd(&closes, 12, 26, 9);
    let macd_line = last_value(&macd.macd_line);
    let signal_line = last_value(&macd.signal_line);
    let histogram = last_value(&macd.histogram);
    let previous_histogram = previous_value(&macd.histogram);
    let average_volume20 = average_last(&volumes, 20);
    let volume_ratio20 = technical::volume_ratio(&volumes, 20);

    let rsi_signal = rsi14.map_or(Signal::Neutral, signals::interpret_rsi);
    let macd_signal = histogram
        .map(|value| signals::interpret_macd(value, previous_histogram))
        .unwrap_or(Signal::Neutral);
    let trend_signal = signals::interpret_trend(latest.close as f64, sma50, sma200);
    let overall = signals::overall_signal(rsi_signal, macd_signal, trend_signal);

    Ok(TechnicalReport {
        symbol: symbol.to_string(),
        as_of: latest.date,
        current_price: latest.close,
        sma20,
        sma50,
        sma200,
        rsi14,
        macd: MacdSnapshot {
            line: macd_line,
            signal: signal_line,
            histogram,
        },
        volume: VolumeSnapshot {
            current: latest.volume,
            average20: average_volume20,
            ratio20: volume_ratio20,
        },
        signals: TechnicalSignal {
            rsi: rsi_signal,
            macd: macd_signal,
            trend: trend_signal,
            overall,
        },
    })
}

fn last_value(values: &[Option<f64>]) -> Option<f64> {
    values.iter().rev().find_map(|value| *value)
}

fn previous_value(values: &[Option<f64>]) -> Option<f64> {
    let mut seen_latest = false;
    for value in values.iter().rev() {
        if value.is_some() {
            if seen_latest {
                return *value;
            }
            seen_latest = true;
        }
    }
    None
}

fn average_last(values: &[f64], period: usize) -> Option<f64> {
    if period == 0 || values.len() < period {
        return None;
    }

    let start = values.len() - period;
    Some(values[start..].iter().sum::<f64>() / period as f64)
}

fn filter_financial_statements(
    financials: &FinancialStatements,
    filters: &FinancialsFilterArgs,
) -> FinancialStatements {
    let includes = |kind: FinancialStatementKind| {
        filters.statement.is_empty() || filters.statement.contains(&kind)
    };

    FinancialStatements {
        instrument: financials.instrument.clone(),
        balance_sheet: includes(FinancialStatementKind::Balance)
            .then(|| financials.balance_sheet.clone())
            .flatten(),
        cash_flow: includes(FinancialStatementKind::Cashflow)
            .then(|| financials.cash_flow.clone())
            .flatten(),
        income_statement: includes(FinancialStatementKind::Income)
            .then(|| financials.income_statement.clone())
            .flatten(),
    }
}

fn filter_earnings_report(report: &EarningsReport, filters: &EarningsFilterArgs) -> EarningsReport {
    let filter_rows = |rows: &[crate::api::types::EarningsData]| {
        rows.iter()
            .filter(|row| filters.includes_period(&row.period_type))
            .cloned()
            .collect()
    };

    EarningsReport {
        symbol: report.symbol.clone(),
        eps_last_year: report.eps_last_year,
        revenue_last_year: report.revenue_last_year,
        forecast: if filters.include_forecast() {
            filter_rows(&report.forecast)
        } else {
            Vec::new()
        },
        history: if filters.include_history() {
            filter_rows(&report.history)
        } else {
            Vec::new()
        },
    }
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

fn screener_filter_key(filter: &str) -> Result<&'static str, IdxError> {
    match filter {
        "top-performers" => Ok("st_list_topperfs"),
        "worst-performers" => Ok("st_list_poorperfs"),
        "high-dividend" => Ok("st_list_highdividend"),
        "low-pe" => Ok("st_list_lowpe"),
        "52w-high" => Ok("st_list_52wkhi"),
        "52w-low" => Ok("st_list_52wklow"),
        "high-volume" => Ok("st_list_topperfs"),
        "large-cap" => Ok("st_list_topperfs"),
        _ => Err(IdxError::InvalidInput(format!(
            "invalid screener filter '{filter}' (expected one of: {})",
            SCREENER_FILTERS.join(", ")
        ))),
    }
}

fn screener_region_key(region: &str) -> Result<&'static str, IdxError> {
    match region {
        "id" => Ok("st_reg_id"),
        "us" => Ok("st_reg_us"),
        "sg" => Ok("st_reg_sg"),
        "hk" => Ok("st_reg_hk"),
        "jp" => Ok("st_reg_jp"),
        _ => Err(IdxError::InvalidInput(format!(
            "invalid screener region '{region}' (expected one of: {})",
            SCREENER_REGIONS.join(", ")
        ))),
    }
}

fn sort_screener_quotes(quotes: &mut [Quote], filter: &str) {
    match filter {
        "top-performers" => {
            quotes.sort_by(|a, b| {
                b.change_pct
                    .partial_cmp(&a.change_pct)
                    .unwrap_or(Ordering::Equal)
            });
        }
        "worst-performers" => {
            quotes.sort_by(|a, b| {
                a.change_pct
                    .partial_cmp(&b.change_pct)
                    .unwrap_or(Ordering::Equal)
            });
        }
        "52w-high" => {
            quotes.sort_by(|a, b| {
                let pa = a.week52_position.unwrap_or(f64::MIN);
                let pb = b.week52_position.unwrap_or(f64::MIN);
                pb.partial_cmp(&pa).unwrap_or(Ordering::Equal)
            });
        }
        "52w-low" => {
            quotes.sort_by(|a, b| {
                let pa = a.week52_position.unwrap_or(f64::MAX);
                let pb = b.week52_position.unwrap_or(f64::MAX);
                pa.partial_cmp(&pb).unwrap_or(Ordering::Equal)
            });
        }
        "high-volume" => {
            quotes.sort_by(|a, b| b.volume.cmp(&a.volume));
        }
        "large-cap" => {
            quotes.sort_by(|a, b| b.market_cap.unwrap_or(0).cmp(&a.market_cap.unwrap_or(0)));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Days, NaiveDate};

    use super::{
        EarningsFilterArgs, EarningsPeriodKind, FinancialStatementKind, FinancialsFilterArgs,
        build_technical_report, classify_earnings_period, filter_earnings_report,
        filter_financial_statements,
    };
    use crate::analysis::signals::Signal;
    use crate::api::types::{
        EarningsData, EarningsReport, FinancialStatements, InstrumentInfo, Ohlc, StatementSection,
    };
    use crate::error::IdxError;

    #[test]
    fn technical_report_uses_latest_values() {
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date");
        let history: Vec<Ohlc> = (0..60)
            .map(|idx| Ohlc {
                date: start
                    .checked_add_days(Days::new(idx as u64))
                    .expect("valid offset"),
                open: 100 + idx as i64,
                high: 101 + idx as i64,
                low: 99 + idx as i64,
                close: 100 + idx as i64,
                volume: 1_000 + idx as u64 * 10,
            })
            .collect();

        let report = build_technical_report("BBCA.JK", &history).expect("report should build");

        assert_eq!(report.symbol, "BBCA.JK");
        assert_eq!(report.current_price, 159);
        assert!(report.sma20.is_some());
        assert!(report.sma50.is_some());
        assert_eq!(report.sma200, None);
        assert!(report.rsi14.is_some());
        assert!(report.volume.ratio20.is_some());
        assert_eq!(report.signals.trend, Signal::Neutral);
    }

    #[test]
    fn sort_screener_top_performers_descending() {
        use crate::api::types::Quote;

        let mut quotes = vec![
            Quote {
                symbol: "A".into(),
                price: 100,
                change: 1,
                change_pct: 1.0,
                volume: 100,
                market_cap: None,
                week52_high: None,
                week52_low: None,
                week52_position: None,
                range_signal: None,
                prev_close: None,
                avg_volume: None,
            },
            Quote {
                symbol: "B".into(),
                price: 200,
                change: 10,
                change_pct: 5.0,
                volume: 200,
                market_cap: None,
                week52_high: None,
                week52_low: None,
                week52_position: None,
                range_signal: None,
                prev_close: None,
                avg_volume: None,
            },
            Quote {
                symbol: "C".into(),
                price: 150,
                change: 5,
                change_pct: 3.0,
                volume: 150,
                market_cap: None,
                week52_high: None,
                week52_low: None,
                week52_position: None,
                range_signal: None,
                prev_close: None,
                avg_volume: None,
            },
        ];

        super::sort_screener_quotes(&mut quotes, "top-performers");
        let pcts: Vec<f64> = quotes.iter().map(|q| q.change_pct).collect();
        assert_eq!(pcts, vec![5.0, 3.0, 1.0]);
    }

    #[test]
    fn sort_screener_worst_performers_ascending() {
        use crate::api::types::Quote;

        let mut quotes = vec![
            Quote {
                symbol: "A".into(),
                price: 100,
                change: 1,
                change_pct: 1.0,
                volume: 100,
                market_cap: None,
                week52_high: None,
                week52_low: None,
                week52_position: None,
                range_signal: None,
                prev_close: None,
                avg_volume: None,
            },
            Quote {
                symbol: "B".into(),
                price: 200,
                change: -10,
                change_pct: -5.0,
                volume: 200,
                market_cap: None,
                week52_high: None,
                week52_low: None,
                week52_position: None,
                range_signal: None,
                prev_close: None,
                avg_volume: None,
            },
            Quote {
                symbol: "C".into(),
                price: 150,
                change: -3,
                change_pct: -2.0,
                volume: 150,
                market_cap: None,
                week52_high: None,
                week52_low: None,
                week52_position: None,
                range_signal: None,
                prev_close: None,
                avg_volume: None,
            },
        ];

        super::sort_screener_quotes(&mut quotes, "worst-performers");
        let pcts: Vec<f64> = quotes.iter().map(|q| q.change_pct).collect();
        assert_eq!(pcts, vec![-5.0, -2.0, 1.0]);
    }

    #[test]
    fn invalid_screener_filter_returns_error() {
        let err = super::screener_filter_key("bogus").expect_err("invalid filter should fail");
        assert!(matches!(err, IdxError::InvalidInput(_)));
        assert!(err.to_string().contains("invalid screener filter"));
    }

    #[test]
    fn invalid_screener_region_returns_error() {
        let err = super::screener_region_key("eu").expect_err("invalid region should fail");
        assert!(matches!(err, IdxError::InvalidInput(_)));
        assert!(err.to_string().contains("invalid screener region"));
    }

    #[test]
    fn filters_financial_statements_to_requested_sections() {
        let sample = FinancialStatements {
            instrument: InstrumentInfo {
                id: "123".into(),
                symbol: "BBCA.JK".into(),
                name: "BCA".into(),
            },
            balance_sheet: Some(StatementSection {
                values: HashMap::from([("assets".into(), 1.0)]),
                currency: "IDR".into(),
                report_date: "2026-01-01".into(),
                end_date: "2025-12-31".into(),
            }),
            cash_flow: Some(StatementSection {
                values: HashMap::from([("cash".into(), 2.0)]),
                currency: "IDR".into(),
                report_date: "2026-01-01".into(),
                end_date: "2025-12-31".into(),
            }),
            income_statement: Some(StatementSection {
                values: HashMap::from([("income".into(), 3.0)]),
                currency: "IDR".into(),
                report_date: "2026-01-01".into(),
                end_date: "2025-12-31".into(),
            }),
        };

        let filtered = filter_financial_statements(
            &sample,
            &FinancialsFilterArgs {
                statement: vec![FinancialStatementKind::Cashflow],
            },
        );

        assert!(filtered.cash_flow.is_some());
        assert!(filtered.income_statement.is_none());
        assert!(filtered.balance_sheet.is_none());
        assert_eq!(filtered.instrument.symbol, "BBCA.JK");
    }

    #[test]
    fn filters_earnings_by_scope_and_period() {
        let report = EarningsReport {
            symbol: "BBCA.JK".into(),
            eps_last_year: 1_200.0,
            revenue_last_year: 100_000_000_000.0,
            forecast: vec![
                EarningsData {
                    eps_actual: None,
                    eps_forecast: Some(1_300.0),
                    eps_surprise: None,
                    eps_surprise_pct: None,
                    revenue_actual: None,
                    revenue_forecast: Some(110_000_000_000.0),
                    revenue_surprise: None,
                    earning_release_date: Some("2026-03-15".into()),
                    period_type: "2026".into(),
                },
                EarningsData {
                    eps_actual: None,
                    eps_forecast: Some(330.0),
                    eps_surprise: None,
                    eps_surprise_pct: None,
                    revenue_actual: None,
                    revenue_forecast: Some(28_000_000_000.0),
                    revenue_surprise: None,
                    earning_release_date: Some("2026-04-20".into()),
                    period_type: "Q12026".into(),
                },
            ],
            history: vec![EarningsData {
                eps_actual: Some(1_250.0),
                eps_forecast: None,
                eps_surprise: Some(20.0),
                eps_surprise_pct: Some(1.6),
                revenue_actual: Some(105_000_000_000.0),
                revenue_forecast: None,
                revenue_surprise: Some(500_000_000.0),
                earning_release_date: Some("2025-03-15".into()),
                period_type: "2025".into(),
            }],
        };

        let filtered = filter_earnings_report(
            &report,
            &EarningsFilterArgs {
                forecast: true,
                history: false,
                annual: true,
                quarterly: false,
            },
        );

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
