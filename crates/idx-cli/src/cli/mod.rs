pub mod cache;
pub mod config;
#[cfg(feature = "ownership")]
pub mod ownership;
pub mod stocks;

use clap::{Parser, Subcommand, ValueEnum};
use idx_core::config::ProviderKind;

use crate::output::OutputSetting;

#[derive(Debug, Clone, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Debug, Parser)]
#[command(
    name = "idx",
    bin_name = "idx",
    version,
    disable_version_flag = true,
    about = "CLI tool for Indonesian stock market (IDX) analysis",
    long_about = "idx-cli is a Rust command-line tool for Indonesian stock market (IDX) analysis.\nIt supports quote lookup, historical data, local caching, and output formats for both humans and AI agents."
)]
pub struct Cli {
    #[arg(
        short,
        long,
        value_enum,
        global = true,
        help = "Output format [default: auto = table on a terminal, json when piped]"
    )]
    pub output: Option<OutputSetting>,
    #[arg(long, global = true, help = "Disable colored output")]
    pub no_color: bool,
    #[arg(short, long, global = true, help = "Suppress non-essential output")]
    pub quiet: bool,
    #[arg(short, long, global = true, action = clap::ArgAction::Count, help = "Increase verbosity (-v, -vv)")]
    pub verbose: u8,
    #[arg(
        long,
        value_enum,
        global = true,
        help = "Market data provider [default: general.provider, else msn]"
    )]
    pub provider: Option<ProviderKind>,
    #[arg(long, global = true, help = "Run without network requests")]
    pub offline: bool,
    #[arg(long, global = true, help = "Bypass local cache")]
    pub no_cache: bool,
    // `idx version` is the documented form; `--version` is a hidden alias.
    #[arg(short = 'V', long, action = clap::ArgAction::Version, hide = true)]
    version: Option<bool>,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Stock data and analysis")]
    Stocks(stocks::StocksCmd),
    #[command(about = "Manage configuration")]
    Config(config::ConfigCmd),
    #[command(about = "Manage local cache")]
    Cache(cache::CacheCmd),
    #[cfg(feature = "ownership")]
    #[command(about = "Ownership intelligence (bootstrap with `ownership sync`)")]
    Ownership(ownership::OwnershipCmd),
    #[command(about = "Generate shell completions")]
    Completions { shell: Shell },
    #[command(about = "Show idx-cli version")]
    Version,
}
