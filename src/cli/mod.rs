pub mod cache;
pub mod config;
#[cfg(feature = "ownership")]
pub mod ownership;
pub mod stocks;

use clap::{Parser, Subcommand, ValueEnum};

use crate::output::OutputFormat;

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
    about = "CLI tool for Indonesian stock market (IDX) analysis",
    long_about = "idx-cli is a Rust command-line tool for Indonesian stock market (IDX) analysis.\nIt supports quote lookup, historical data, local caching, and output formats for both humans and AI agents."
)]
pub struct Cli {
    #[arg(short, long, value_enum, global = true, help = "Output format")]
    pub output: Option<OutputFormat>,
    #[arg(long, global = true, help = "Disable colored output")]
    pub no_color: bool,
    #[arg(short, long, global = true, help = "Suppress non-essential output")]
    pub quiet: bool,
    #[arg(short, long, global = true, action = clap::ArgAction::Count, help = "Increase verbosity (-v, -vv)")]
    pub verbose: u8,
    #[arg(long, global = true, help = "Run without network requests")]
    pub offline: bool,
    #[arg(long, global = true, help = "Bypass local cache")]
    pub no_cache: bool,
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
    #[command(about = "Ownership intelligence (KSEI + Bing)")]
    Ownership(ownership::OwnershipCmd),
    #[command(about = "Generate shell completions")]
    Completions { shell: Shell },
    #[command(about = "Show idx-cli version")]
    Version,
}
