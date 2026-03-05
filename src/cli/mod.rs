pub mod cache;
pub mod config;
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
#[command(name = "idx", about = "Indonesian stock analysis CLI")]
pub struct Cli {
    #[arg(short, long, value_enum, global = true)]
    pub output: Option<OutputFormat>,
    #[arg(long, global = true)]
    pub no_color: bool,
    #[arg(short, long, global = true)]
    pub quiet: bool,
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    #[arg(long, global = true)]
    pub offline: bool,
    #[arg(long, global = true)]
    pub no_cache: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Stocks(stocks::StocksCmd),
    Config(config::ConfigCmd),
    Cache(cache::CacheCmd),
    Completions { shell: Shell },
    Version,
}
