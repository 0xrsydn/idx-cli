use clap::{Args, Subcommand};

use crate::analysis::signals::{self, Signal, TechnicalSignal};
use crate::analysis::technical;
use crate::api::MarketDataProvider;
use crate::api::types::{Interval, Ohlc, Period};
use crate::cache::Cache;
use crate::config::IdxConfig;
use crate::error::IdxError;
use crate::output::{
    MacdSnapshot, TechnicalReport, VolumeSnapshot, render_history, render_quotes, render_technical,
};

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
            let mut quotes = Vec::new();
            for sym in symbols.iter().flat_map(|s| s.split(',')) {
                let resolved = crate::api::resolve_symbol(sym, &config.exchange);
                if !no_cache && let Some(q) = cache.get("quote", &resolved)? {
                    quotes.push(q);
                    continue;
                }
                if offline {
                    let stale = cache
                        .get_stale("quote", &resolved)?
                        .ok_or_else(|| IdxError::CacheMiss(format!("quote/{resolved}")))?;
                    quotes.push(stale);
                    continue;
                }

                match provider.quote(&resolved) {
                    Ok(q) => {
                        if !no_cache {
                            cache.put("quote", &resolved, &q, config.quote_ttl)?;
                        }
                        quotes.push(q);
                    }
                    Err(err) => {
                        if !no_cache && let Some(stale) = cache.get_stale("quote", &resolved)? {
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
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let key = format!("{}-{}", period.as_str(), interval.as_str());
            if !no_cache
                && let Some(history) = cache
                    .get::<Vec<crate::api::types::Ohlc>>("history", &format!("{resolved}-{key}"))?
            {
                return render_history(&resolved, &history, &config.output);
            }
            if offline {
                let stale = cache
                    .get_stale::<Vec<crate::api::types::Ohlc>>(
                        "history",
                        &format!("{resolved}-{key}"),
                    )?
                    .ok_or_else(|| IdxError::CacheMiss(format!("history/{resolved}-{key}")))?;
                return render_history(&resolved, &stale, &config.output);
            }

            match provider.history(&resolved, period, interval) {
                Ok(history) => {
                    if !no_cache {
                        cache.put(
                            "history",
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
                            "history",
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
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            if !no_cache
                && let Some(report) = cache.get::<TechnicalReport>("technical", &resolved)?
            {
                return render_technical(&report, &config.output, config.no_color);
            }
            if offline {
                let stale = cache
                    .get_stale::<TechnicalReport>("technical", &resolved)?
                    .ok_or_else(|| IdxError::CacheMiss(format!("technical/{resolved}")))?;
                return render_technical(&stale, &config.output, config.no_color);
            }

            match provider.history(&resolved, &Period::OneYear, &Interval::Day) {
                Ok(history) => {
                    let report = build_technical_report(&resolved, &history)?;
                    if !no_cache {
                        cache.put("technical", &resolved, &report, config.quote_ttl)?;
                    }
                    render_technical(&report, &config.output, config.no_color)
                }
                Err(err) => {
                    if !no_cache
                        && let Some(stale) =
                            cache.get_stale::<TechnicalReport>("technical", &resolved)?
                    {
                        eprintln!("warning: network failed, serving stale cache for {resolved}");
                        return render_technical(&stale, &config.output, config.no_color);
                    }
                    Err(err)
                }
            }
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
