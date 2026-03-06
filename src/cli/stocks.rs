use clap::{Args, Subcommand};
use serde::{Serialize, de::DeserializeOwned};

use crate::analysis::fundamental::{
    FundamentalReport, GrowthReport, RiskReport, ValuationReport, analyze_fundamental,
    analyze_growth, analyze_risk, analyze_valuation,
};
use crate::analysis::signals::{self, Signal, TechnicalSignal};
use crate::analysis::technical;
use crate::api::msn::MsnProvider;
use crate::api::types::{
    CompanyProfile, EarningsReport, FinancialStatements, Fundamentals, InsightData, Interval,
    NewsItem, Ohlc, Period, Quote, SentimentData,
};
use crate::api::{
    EarningsProvider, FinancialsProvider, InsightsProvider, MarketDataProvider, NewsProvider,
    ProfileProvider, SentimentProvider, history_provider,
};
use crate::cache::Cache;
use crate::config::IdxConfig;
use crate::error::IdxError;
use crate::output::{
    MacdSnapshot, TechnicalReport, VolumeSnapshot, render_compare, render_earnings,
    render_financials, render_fundamental, render_growth, render_history, render_insights,
    render_news, render_profile, render_quotes, render_risk, render_screener, render_sentiment,
    render_technical, render_valuation,
};

struct FundamentalCacheSpec {
    bucket: String,
    ttl_secs: u64,
}

fn cache_bucket(config: &IdxConfig, key: &str) -> String {
    format!("{}-{key}", config.provider.as_str())
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
    },
    #[command(
        about = "Run technical analysis on a stock",
        after_help = "Examples:\n  idx stocks technical BBCA\n  idx -o json stocks technical BBCA"
    )]
    Technical {
        /// Single ticker symbol (e.g. BBCA).
        symbol: String,
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
    #[command(about = "Get financial statements")]
    Financials {
        symbol: String,
        #[arg(long, default_value = "income")]
        statement: String,
    },
    #[command(about = "Get earnings report")]
    Earnings {
        symbol: String,
        #[arg(long)]
        annual: bool,
        #[arg(long)]
        quarterly: bool,
        #[arg(long)]
        forecast: bool,
        #[arg(long)]
        history: bool,
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
    provider: &dyn MarketDataProvider,
    offline: bool,
    no_cache: bool,
) -> Result<(), IdxError> {
    let cache = Cache::new()?;

    match &cmd.command {
        StocksSubcommand::Quote { symbols } => {
            let quote_bucket = cache_bucket(config, "quote");
            let mut quotes = Vec::new();
            for sym in symbols.iter().flat_map(|s| s.split(',')) {
                let resolved = crate::api::resolve_symbol(sym, &config.exchange);
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

                match provider.quote(&resolved) {
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
                            eprintln!(
                                "warning: network failed, serving stale cache for {resolved}"
                            );
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
        } => {
            let hist_provider = history_provider(config.provider, false).ok_or_else(|| {
                IdxError::Unsupported(
                    "MSN does not provide price history for IDX stocks. \
                         Use --provider yahoo for historical data."
                        .into(),
                )
            })?;
            let history_bucket = cache_bucket(config, "history");
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
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
                        eprintln!("warning: network failed, serving stale cache for {resolved}");
                        return render_history(&resolved, &stale, &config.output);
                    }
                    Err(err)
                }
            }
        }
        StocksSubcommand::Technical { symbol } => {
            let hist_provider = history_provider(config.provider, false).ok_or_else(|| {
                IdxError::Unsupported(
                    "MSN does not provide price history for IDX stocks. \
                         Use --provider yahoo for technical analysis."
                        .into(),
                )
            })?;
            let technical_bucket = cache_bucket(config, "technical");
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
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
                        eprintln!("warning: network failed, serving stale cache for {resolved}");
                        return render_technical(&stale, &config.output, config.no_color);
                    }
                    Err(err)
                }
            }
        }
        StocksSubcommand::Growth { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let report: GrowthReport = fetch_fundamental_analysis_report(
                &cache,
                provider,
                &resolved,
                FundamentalCacheSpec {
                    bucket: cache_bucket(config, "growth"),
                    ttl_secs: config.fundamental_ttl,
                },
                offline,
                no_cache,
                |_, fundamentals| analyze_growth(fundamentals),
            )?;
            render_growth(&resolved, &report, &config.output, config.no_color)
        }
        StocksSubcommand::Valuation { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let report: ValuationReport = fetch_fundamental_analysis_report(
                &cache,
                provider,
                &resolved,
                FundamentalCacheSpec {
                    bucket: cache_bucket(config, "valuation"),
                    ttl_secs: config.fundamental_ttl,
                },
                offline,
                no_cache,
                |_, fundamentals| analyze_valuation(fundamentals),
            )?;
            render_valuation(&resolved, &report, &config.output, config.no_color)
        }
        StocksSubcommand::Risk { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let report: RiskReport = fetch_fundamental_analysis_report(
                &cache,
                provider,
                &resolved,
                FundamentalCacheSpec {
                    bucket: cache_bucket(config, "risk"),
                    ttl_secs: config.fundamental_ttl,
                },
                offline,
                no_cache,
                |_, fundamentals| analyze_risk(fundamentals),
            )?;
            render_risk(&resolved, &report, &config.output, config.no_color)
        }
        StocksSubcommand::Fundamental { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let report: FundamentalReport = fetch_fundamental_analysis_report(
                &cache,
                provider,
                &resolved,
                FundamentalCacheSpec {
                    bucket: cache_bucket(config, "fundamental"),
                    ttl_secs: config.fundamental_ttl,
                },
                offline,
                no_cache,
                analyze_fundamental,
            )?;
            render_fundamental(&report, &config.output, config.no_color)
        }
        StocksSubcommand::Profile { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let profile: CompanyProfile = fetch_msn_only(&resolved, config.provider, || {
                MsnProvider::new(false).profile(&resolved)
            })?;
            render_profile(&profile, &config.output)
        }
        StocksSubcommand::Financials {
            symbol,
            statement: _,
        } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let financials: FinancialStatements =
                fetch_msn_only(&resolved, config.provider, || {
                    MsnProvider::new(false).financials(&resolved)
                })?;
            render_financials(&financials, &config.output)
        }
        StocksSubcommand::Earnings {
            symbol,
            annual: _,
            quarterly: _,
            forecast: _,
            history: _,
        } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let earnings: EarningsReport = fetch_msn_only(&resolved, config.provider, || {
                MsnProvider::new(false).earnings(&resolved)
            })?;
            render_earnings(&earnings, &config.output)
        }
        StocksSubcommand::Sentiment { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let sentiment: SentimentData = fetch_msn_only(&resolved, config.provider, || {
                MsnProvider::new(false).sentiment(&resolved)
            })?;
            render_sentiment(&sentiment, &config.output)
        }
        StocksSubcommand::Insights { symbol } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let insights: InsightData = fetch_msn_only(&resolved, config.provider, || {
                MsnProvider::new(false).insights(&resolved)
            })?;
            render_insights(&insights, &config.output)
        }
        StocksSubcommand::News { symbol, limit } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let news: Vec<NewsItem> = fetch_msn_only(&resolved, config.provider, || {
                MsnProvider::new(false).news(&resolved, *limit)
            })?;
            render_news(&news, &config.output)
        }
        StocksSubcommand::Screen {
            filter,
            region,
            limit,
        } => {
            let msn = MsnProvider::new(false);
            let filter_key = screener_filter_key(filter);
            let region_key = screener_region_key(region);
            let quotes: Vec<Quote> = fetch_msn_only("screen", config.provider, || {
                msn.screener(filter_key, region_key, *limit)
            })?;
            render_screener(&quotes, &config.output, config.no_color)
        }
        StocksSubcommand::Compare { symbols } => {
            let mut reports: Vec<FundamentalReport> = Vec::new();
            let mut last_error = None;

            for sym in symbols.iter().flat_map(|s| s.split(',')) {
                let resolved = crate::api::resolve_symbol(sym, &config.exchange);
                match fetch_fundamental_analysis_report(
                    &cache,
                    provider,
                    &resolved,
                    FundamentalCacheSpec {
                        bucket: cache_bucket(config, "fundamental"),
                        ttl_secs: config.fundamental_ttl,
                    },
                    offline,
                    no_cache,
                    analyze_fundamental,
                ) {
                    Ok(report) => reports.push(report),
                    Err(err) => {
                        eprintln!("warning: failed to fetch fundamentals for {resolved}: {err}");
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

#[allow(dead_code)] // wired up once per-subcommand handlers are fully split
pub(crate) fn fetch_with_cache<T, F>(
    cache: &Cache,
    bucket: &str,
    key: &str,
    ttl_secs: u64,
    offline: bool,
    no_cache: bool,
    fetch_fn: F,
) -> Result<T, IdxError>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce() -> Result<T, IdxError>,
{
    if !no_cache
        && !offline
        && let Some(cached) = cache.get::<T>(bucket, key)?
    {
        return Ok(cached);
    }

    if offline {
        return cache
            .get_stale::<T>(bucket, key)?
            .ok_or_else(|| IdxError::Offline("no cached data available".to_string()));
    }

    let data = fetch_fn()?;
    if !no_cache {
        let _ = cache.put(bucket, key, &data, ttl_secs);
    }
    Ok(data)
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
                eprintln!("warning: network failed, serving stale cache for {resolved}");
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

fn fetch_msn_only<T>(
    symbol: &str,
    provider: crate::config::ProviderKind,
    f: impl FnOnce() -> Result<T, IdxError>,
) -> Result<T, IdxError> {
    if !matches!(provider, crate::config::ProviderKind::Msn) {
        return Err(IdxError::Unsupported(format!(
            "{symbol}: command requires --provider msn"
        )));
    }
    f()
}

fn screener_filter_key(filter: &str) -> &'static str {
    match filter {
        "top-performers" => "st_list_topperfs",
        "worst-performers" => "st_list_poorperfs",
        "high-dividend" => "st_list_highdividend",
        "low-pe" => "st_list_lowpe",
        "52w-high" => "st_list_52wkhi",
        "52w-low" => "st_list_52wklow",
        "high-volume" => "st_list_highvol",
        "large-cap" => "st_list_largecap",
        _ => "st_list_topperfs",
    }
}

fn screener_region_key(region: &str) -> &'static str {
    match region {
        "id" => "st_reg_id",
        "us" => "st_reg_us",
        "sg" => "st_reg_sg",
        "hk" => "st_reg_hk",
        "jp" => "st_reg_jp",
        _ => "st_reg_id",
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Days, NaiveDate};

    use super::build_technical_report;
    use crate::analysis::signals::Signal;
    use crate::api::types::Ohlc;

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
}
