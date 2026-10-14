mod cli;
mod config;
mod output;

use idx_core::runtime;

use clap::CommandFactory;
use clap::Parser;
use clap_complete::{generate, shells};

use crate::cli::{Cli, Commands};
use crate::config::IdxConfig;
use crate::output::{OutputFormat, OutputSetting, emit_error};
use idx_core::error::IdxError;

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
    output::json::configure(config.output, config::stdout_is_terminal());
    output::style::set_color(!config.no_color);

    match &cli.command {
        Commands::Version => {
            if config.output.is_json() {
                output::json::print_json(&serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION")
                }))?;
            } else {
                println!("{}", env!("CARGO_PKG_VERSION"));
            }
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
            if let Err(err) = cli::stocks::handle(stocks, &config, cli.offline, cli.no_cache) {
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
            if let Err(err) = cli::cache::handle(cache, &config.output) {
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

/// Output format for errors raised before the configuration has loaded.
fn bootstrap_output_format(cli: &Cli) -> OutputFormat {
    let setting = cli
        .output
        .or_else(|| {
            std::env::var("IDX_OUTPUT")
                .ok()
                .and_then(|value| OutputSetting::parse(&value))
        })
        .unwrap_or_default();
    setting.resolve(config::stdout_is_terminal())
}
