use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct CacheCmd {
    #[command(subcommand)]
    pub command: CacheSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CacheSubcommand {
    Info,
    Clear,
}
