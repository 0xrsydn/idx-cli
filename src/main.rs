pub mod analysis;
mod api;
mod cache;
mod cli;
mod config;
mod curl_impersonate;
mod error;
mod output;
#[cfg(feature = "ownership")]
pub mod ownership;

pub mod runtime {
    use std::fmt::Display;
    use std::sync::atomic::{AtomicBool, Ordering};

    static QUIET: AtomicBool = AtomicBool::new(false);

    pub fn set_quiet(quiet: bool) {
        QUIET.store(quiet, Ordering::Relaxed);
    }

    pub fn is_quiet() -> bool {
        QUIET.load(Ordering::Relaxed)
    }

    pub fn info(message: impl Display) {
        if !is_quiet() {
            eprintln!("info: {message}");
        }
    }

    pub fn warn(message: impl Display) {
        if !is_quiet() {
            eprintln!("warning: {message}");
        }
    }
}

use clap::CommandFactory;
use clap::Parser;
use clap_complete::{generate, shells};

use crate::api::default_provider;
use crate::cli::{Cli, Commands};
use crate::config::IdxConfig;
use crate::error::IdxError;
use crate::output::{OutputFormat, emit_error};

fn main() {
    if let Err(err) = run() {
        std::process::exit(err.exit_code());
    }
}

fn run() -> Result<(), IdxError> {
    let cli = Cli::parse();
    runtime::set_quiet(cli.quiet);
    let config = match IdxConfig::load_with_cli(&cli) {
        Ok(config) => config,
        Err(err) => {
            emit_error(&err, &bootstrap_output_format(&cli));
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
                &provider,
                cli.offline,
                cli.no_cache,
                cli.verbose > 0,
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
        #[cfg(feature = "ownership")]
        Commands::Ownership(cmd) => {
            if let Err(err) = cli::ownership::handle(&cmd.command, &config) {
                emit_error(&err, &config.output);
                return Err(err);
            }
        }
    }

    Ok(())
}

fn bootstrap_output_format(cli: &Cli) -> OutputFormat {
    if let Some(output) = cli.output {
        return output;
    }

    match std::env::var("IDX_OUTPUT") {
        Ok(value) if value.eq_ignore_ascii_case("json") => OutputFormat::Json,
        _ => OutputFormat::Table,
    }
}
