use std::io::Read;

use clap::{Args, Subcommand};

use crate::config::{HistoryProviderKind, IdxConfig};
use crate::output::{
    render_compare, render_earnings, render_financials, render_fundamental, render_growth,
    render_history, render_insights, render_news, render_profile, render_quotes, render_risk,
    render_screener, render_sentiment, render_technical, render_valuation,
};
use idx_core::api::types::{Interval, Period};
use idx_core::error::IdxError;
use idx_core::financial::{EarningsFilter, StatementFilter, StatementKind};
use idx_core::market::ScreenQuery;
use idx_core::{CachePolicy, Idx, runtime};

/// Splits comma-separated symbol arguments; `-` reads symbols from stdin,
/// separated by whitespace or commas.
fn expand_symbols(args: &[String]) -> Result<Vec<String>, IdxError> {
    let mut symbols = Vec::new();
    for arg in args {
        if arg == "-" {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| IdxError::Io(e.to_string()))?;
            let from_stdin: Vec<String> = input
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            if from_stdin.is_empty() {
                return Err(IdxError::InvalidInput(
                    "no symbols read from stdin".to_string(),
                ));
            }
            symbols.extend(from_stdin);
        } else {
            symbols.extend(arg.split(',').map(String::from));
        }
    }
    Ok(symbols)
}

#[derive(Debug, Clone, Args, Default)]
pub struct FinancialsFilterArgs {
    /// Limit output to one or more statements.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    statement: Vec<StatementKind>,
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

impl From<EarningsFilterArgs> for EarningsFilter {
    fn from(args: EarningsFilterArgs) -> Self {
        Self {
            forecast: args.forecast,
            history: args.history,
            annual: args.annual,
            quarterly: args.quarterly,
        }
    }
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
        after_help = "Examples:\n  idx stocks quote BBCA\n  idx stocks quote BBCA,BBRI,BMRI\n  idx -o json stocks quote BBCA\n  echo BBCA BBRI | idx stocks quote -"
    )]
    Quote {
        /// One or more symbols, comma-separated or space-separated; `-` reads them from stdin.
        #[arg(required = true)]
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
        after_help = "Examples:\n  idx stocks compare BBCA BBRI BMRI\n  idx stocks compare BBCA,BBRI,BMRI\n  idx -o json stocks compare BBCA,BBRI\n  idx stocks screen --filter low-pe | jq -r '.[].symbol' | idx stocks compare -"
    )]
    Compare {
        /// One or more symbols, comma-separated or space-separated; `-` reads them from stdin.
        #[arg(required = true)]
        symbols: Vec<String>,
    },
}

pub fn handle(
    cmd: &StocksCmd,
    config: &IdxConfig,
    offline: bool,
    no_cache: bool,
) -> Result<(), IdxError> {
    let policy = CachePolicy::from_flags(offline, no_cache)?;
    let idx = Idx::new(config.core.clone())?;
    let format = &config.output;
    let no_color = config.no_color;

    match &cmd.command {
        StocksSubcommand::Quote { symbols } => {
            let quotes = idx.market().quotes(&expand_symbols(symbols)?, policy)?;
            let quotes: Vec<_> = quotes.into_iter().map(|q| q.data).collect();
            render_quotes(&quotes, format, no_color)
        }
        StocksSubcommand::History {
            symbol,
            period,
            interval,
            history_provider,
        } => {
            let source = idx.market().history_source(*history_provider);
            announce_history_fallback(&idx, source, config);
            let history = idx
                .market()
                .history(symbol, period, interval, source, policy)?;
            let resolved = idx_core::api::resolve_symbol(symbol, &config.core.exchange)?;
            render_history(&resolved, &history.data, format)
        }
        StocksSubcommand::Technical {
            symbol,
            history_provider,
        } => {
            let source = idx.market().history_source(*history_provider);
            announce_history_fallback(&idx, source, config);
            let report = idx.analysis().technical(symbol, source, policy)?;
            render_technical(&report.data, format, no_color)
        }
        StocksSubcommand::Growth { symbol } => {
            let report = idx.analysis().growth(symbol, policy)?;
            render_growth(&resolve(symbol, config)?, &report.data, format, no_color)
        }
        StocksSubcommand::Valuation { symbol } => {
            let report = idx.analysis().valuation(symbol, policy)?;
            render_valuation(&resolve(symbol, config)?, &report.data, format, no_color)
        }
        StocksSubcommand::Risk { symbol } => {
            let report = idx.analysis().risk(symbol, policy)?;
            render_risk(&resolve(symbol, config)?, &report.data, format, no_color)
        }
        StocksSubcommand::Fundamental { symbol } => {
            let report = idx.analysis().fundamental(symbol, policy)?;
            render_fundamental(&report.data, format, no_color)
        }
        StocksSubcommand::Profile { symbol } => {
            let profile = idx.financial().profile(symbol, policy)?;
            render_profile(&profile.data, format)
        }
        StocksSubcommand::Financials { symbol, filters } => {
            let filter = StatementFilter {
                statements: filters.statement.clone(),
            };
            let financials = idx.financial().statements(symbol, &filter, policy)?;
            render_financials(&financials.data, format)
        }
        StocksSubcommand::Earnings { symbol, filters } => {
            let earnings =
                idx.financial()
                    .earnings(symbol, EarningsFilter::from(*filters), policy)?;
            render_earnings(&earnings.data, format)
        }
        StocksSubcommand::Sentiment { symbol } => {
            let sentiment = idx.financial().sentiment(symbol, policy)?;
            render_sentiment(&sentiment.data, format)
        }
        StocksSubcommand::Insights { symbol } => {
            let insights = idx.financial().insights(symbol, policy)?;
            render_insights(&insights.data, format)
        }
        StocksSubcommand::News { symbol, limit } => {
            let news = idx.financial().news(symbol, *limit, policy)?;
            render_news(&news.data, format)
        }
        StocksSubcommand::Screen {
            filter,
            region,
            limit,
        } => {
            let query = ScreenQuery {
                filter: filter.parse()?,
                region: region.parse()?,
                limit: *limit,
            };
            let quotes = idx.market().screen(&query, policy)?;
            render_screener(&quotes.data, format, no_color)
        }
        StocksSubcommand::Compare { symbols } => {
            let comparison = idx.analysis().compare(&expand_symbols(symbols)?, policy)?;
            // Partial success needs diagnostics; total failure must leave stderr
            // available for the single structured error emitted by the caller.
            for (resolved, err) in comparison.failures {
                runtime::warn(format!(
                    "failed to fetch fundamentals for {resolved}: {err}"
                ));
            }
            let reports: Vec<_> = comparison.reports.into_iter().map(|r| r.data).collect();
            render_compare(&reports, format, no_color)
        }
    }
}

fn resolve(symbol: &str, config: &IdxConfig) -> Result<String, IdxError> {
    idx_core::api::resolve_symbol(symbol, &config.core.exchange)
}

/// Table mode notes when `auto` history falls back to another provider.
fn announce_history_fallback(
    idx: &Idx,
    source: idx_core::market::HistorySource,
    config: &IdxConfig,
) {
    if source.is_fallback && !config.output.is_json() {
        runtime::info(format!(
            "history provider fallback active ({} -> {})",
            idx.config().provider.as_str(),
            source.provider.as_str()
        ));
    }
}
