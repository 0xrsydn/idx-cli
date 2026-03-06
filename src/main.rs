pub mod analysis;
mod api;
mod cache;
mod cli;
mod config;
mod error;
mod output;

use clap::CommandFactory;
use clap::Parser;
use clap_complete::{generate, shells};

use crate::api::default_provider;
use crate::cli::{Cli, Commands};
use crate::config::IdxConfig;
use crate::error::IdxError;
use crate::output::emit_error;

fn main() {
    if let Err(err) = run() {
        std::process::exit(err.exit_code());
    }
}

fn run() -> Result<(), IdxError> {
    let cli = Cli::parse();
    let config = match IdxConfig::load_with_cli(&cli) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error: {err}");
            return Err(err);
        }
    };

    match &cli.command {
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_owned();
            match shell {
                cli::Shell::Bash => generate(shells::Bash, &mut cmd, name, &mut std::io::stdout()),
                cli::Shell::Zsh => generate(shells::Zsh, &mut cmd, name, &mut std::io::stdout()),
                cli::Shell::Fish => generate(shells::Fish, &mut cmd, name, &mut std::io::stdout()),
            }
        }
        Commands::Stocks(stocks) => {
            let provider = default_provider(config.provider, cli.verbose > 0);
            if let Err(err) = cli::stocks::handle(
                stocks,
                &config,
                provider.as_ref(),
                cli.offline,
                cli.no_cache,
            ) {
                emit_error(&err, &config.output);
                return Err(err);
            }
        }
        Commands::Config(cfg) => {
            if let Err(err) = cli::config::handle(cfg) {
                emit_error(&err, &config.output);
                return Err(err);
            }
        }
        Commands::Cache(cache) => {
            if let Err(err) = cli::cache::handle(cache) {
                emit_error(&err, &config.output);
                return Err(err);
            }
        }
    }

    Ok(())
}
