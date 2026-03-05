use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ConfigCmd {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    Init,
    Get { key: String },
    Set { key: String, value: String },
    Path,
}
