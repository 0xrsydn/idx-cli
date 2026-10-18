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
            if let Err(err) = cli::config::handle(cfg, config.output_setting) {
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
/// `-o`/`--output` as clap reads it (clustered short flags, the last one
/// at any subcommand level), then `IDX_OUTPUT`, then auto.
fn requested_output_from_args() -> OutputFormat {
    // args_os: an argument that is not UTF-8 (itself the error being
    // reported) must not panic here.
    let setting = output_flag(std::env::args_os().collect())
        .or_else(|| {
            std::env::var("IDX_OUTPUT")
                .ok()
                .and_then(|value| OutputSetting::parse(&value))
        })
        .unwrap_or_default();
    setting.resolve(config::stdout_is_terminal())
}

/// clap stops at an unknown argument, so an `-o` after it would be missed;
/// drop each unknown argument and parse again. Each pass is read with
/// ignore_errors, which keeps the matches made before any other error; the
/// last pass that read an `-o` wins (a later pass can reach `--help`, which
/// returns no matches at all).
fn output_flag(mut args: Vec<std::ffi::OsString>) -> Option<OutputSetting> {
    use clap::CommandFactory;
    use clap::error::{ContextKind, ContextValue, ErrorKind};
    let read = |args: &[std::ffi::OsString]| {
        Cli::command()
            .ignore_errors(true)
            .try_get_matches_from(args)
            .ok()
            .and_then(|matches| matches.get_one::<OutputSetting>("output").copied())
    };
    let mut found = read(&args);
    for _ in 0..args.len() {
        let Err(err) = Cli::command().try_get_matches_from(&args) else {
            break;
        };
        if err.kind() != ErrorKind::UnknownArgument {
            break;
        }
        let Some(ContextValue::String(bad)) = err.get(ContextKind::InvalidArg) else {
            break;
        };
        let Some(index) = args.iter().skip(1).position(|arg| {
            let arg = arg.to_string_lossy();
            arg == bad.as_str() || arg.starts_with(&format!("{bad}="))
        }) else {
            break;
        };
        args.remove(index + 1);
        found = read(&args).or(found);
    }
    found
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
