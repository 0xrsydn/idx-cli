use clap::{Args, Subcommand};

use crate::api::types::{Interval, Period};
use crate::api::MarketDataProvider;
use crate::cache::Cache;
use crate::config::IdxConfig;
use crate::error::IdxError;
use crate::output::{render_history, render_quotes};

#[derive(Debug, Args)]
pub struct StocksCmd {
    #[command(subcommand)]
    pub command: StocksSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum StocksSubcommand {
    Quote { symbols: Vec<String> },
    History {
        symbol: String,
        #[arg(long, value_enum, default_value_t = Period::ThreeMonths)]
        period: Period,
        #[arg(long, value_enum, default_value_t = Interval::Day)]
        interval: Interval,
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
                if !no_cache
                    && let Some(q) = cache.get("quote", &resolved)?
                {
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
                        if !no_cache
                            && let Some(stale) = cache.get_stale("quote", &resolved)?
                        {
                            eprintln!("warning: network failed, serving stale cache for {resolved}");
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
                && let Some(history) = cache.get::<Vec<crate::api::types::Ohlc>>("history", &format!("{resolved}-{key}"))?
            {
                return render_history(&resolved, &history, &config.output);
            }
            if offline {
                let stale = cache
                    .get_stale::<Vec<crate::api::types::Ohlc>>("history", &format!("{resolved}-{key}"))?
                    .ok_or_else(|| IdxError::CacheMiss(format!("history/{resolved}-{key}")))?;
                return render_history(&resolved, &stale, &config.output);
            }

            match provider.history(&resolved, period, interval) {
                Ok(history) => {
                    if !no_cache {
                        cache.put("history", &format!("{resolved}-{key}"), &history, config.quote_ttl)?;
                    }
                    render_history(&resolved, &history, &config.output)
                }
                Err(err) => {
                    if !no_cache
                        && let Some(stale) = cache.get_stale::<Vec<crate::api::types::Ohlc>>("history", &format!("{resolved}-{key}"))?
                    {
                        eprintln!("warning: network failed, serving stale cache for {resolved}");
                        return render_history(&resolved, &stale, &config.output);
                    }
                    Err(err)
                }
            }
        }
    }
}
