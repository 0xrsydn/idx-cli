use clap::{Args, Subcommand};

use crate::api::types::{Interval, Period};
use crate::api::MarketDataProvider;
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
) -> Result<(), IdxError> {
    match &cmd.command {
        StocksSubcommand::Quote { symbols } => {
            let mut quotes = Vec::new();
            for sym in symbols.iter().flat_map(|s| s.split(',')) {
                let resolved = crate::api::resolve_symbol(sym, &config.exchange);
                quotes.push(provider.quote(&resolved)?);
            }
            render_quotes(&quotes, &config.output, config.no_color)
        }
        StocksSubcommand::History {
            symbol,
            period,
            interval,
        } => {
            let resolved = crate::api::resolve_symbol(symbol, &config.exchange);
            let history = provider.history(&resolved, period, interval)?;
            render_history(&resolved, &history, &config.output)
        }
    }
}
