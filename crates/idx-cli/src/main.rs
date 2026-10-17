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
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => return Err(argument_error(err)),
    };
    runtime::set_quiet(cli.quiet);
    let config = match IdxConfig::load_with_cli(&cli) {
        Ok(config) => config,
        Err(err) => {
            emit_error(&err, &bootstrap_output_format(&cli));
            return Err(err);
        }
    };
    output::json::configure(config.output, config::stdout_is_terminal());
    runtime::set_json(config.output.is_json());
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
            if let Err(err) = cli::config::handle(cfg, &config.output) {
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
/// Argument errors honor the output contract: in JSON mode they are one JSON
/// error on stderr like any other failure (clap prints plain text). Help and
/// `--version` print as usual.
fn argument_error(err: clap::Error) -> IdxError {
    use clap::error::ErrorKind;
    // A missing subcommand prints help but is an error (exit 2), so in JSON
    // mode it is a JSON error too.
    if matches!(
        err.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) || !requested_output_from_args().is_json()
    {
        err.exit();
    }
    let rendered = err.render().to_string();
    let message = rendered
        .trim()
        .strip_prefix("error: ")
        .unwrap_or(rendered.trim())
        .to_string();
    let error = IdxError::InvalidInput(message);
    emit_error(&error, &requested_output_from_args());
    error
}

/// The output format the raw arguments ask for, when they did not parse:
/// `-o`/`--output` (any spelling), then `IDX_OUTPUT`, then auto.
fn requested_output_from_args() -> OutputFormat {
    // args_os: an argument that is not UTF-8 (itself the error being
    // reported) must not panic here.
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let flag = args.iter().enumerate().find_map(|(i, arg)| {
        if arg == "-o" || arg == "--output" {
            args.get(i + 1).cloned()
        } else if let Some(value) = arg.strip_prefix("--output=") {
            Some(value.to_string())
        } else if arg.starts_with("-o") && arg.len() > 2 {
            Some(arg[2..].trim_start_matches('=').to_string())
        } else {
            None
        }
    });
    let setting = flag
        .and_then(|value| OutputSetting::parse(&value))
        .or_else(|| {
            std::env::var("IDX_OUTPUT")
                .ok()
                .and_then(|value| OutputSetting::parse(&value))
        })
        .unwrap_or_default();
    setting.resolve(config::stdout_is_terminal())
}

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
