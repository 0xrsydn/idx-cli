use clap::{Args, Subcommand};

use crate::config::{config_path, ensure_default_config, get_config_value, set_config_value};
use crate::output::{OutputSetting, json};
use idx_core::error::IdxError;

#[derive(Debug, Args)]
#[command(
    about = "Manage configuration",
    long_about = "Manage configuration in ~/.config/idx/config.toml (or $XDG_CONFIG_HOME/idx/config.toml).\n\nResults print as plain text, even when piped, so `$(idx config path)` works in scripts. Ask for JSON with `-o json`, `IDX_OUTPUT=json`, or `general.output = \"json\"`.",
    after_help = "Keys:\n  general.provider, general.history_provider, general.exchange, general.output,\n  general.color, cache.quote_ttl, cache.fundamental_ttl, ownership.db_path,\n  ownership.snapshot_manifest\n\nExamples:\n  idx config init\n  idx config set general.output json\n  idx config get cache.quote_ttl\n  idx config path"
)]
pub struct ConfigCmd {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    #[command(about = "Create default configuration file")]
    Init,
    #[command(about = "Get a configuration value by key")]
    Get {
        /// Key such as general.provider (see `idx config --help`).
        key: String,
    },
    #[command(about = "Set a configuration value by key")]
    Set {
        /// Key such as general.provider (see `idx config --help`).
        key: String,
        value: String,
    },
    #[command(about = "Print configuration file path")]
    Path,
}

pub fn handle(cmd: &ConfigCmd, setting: OutputSetting) -> Result<(), IdxError> {
    // Plain text, as scripts read `$(idx config path)`, unless JSON is asked
    // for (-o json, IDX_OUTPUT, or general.output); auto stays text when piped.
    let print = |text: String, document: serde_json::Value| {
        if matches!(setting, OutputSetting::Json | OutputSetting::Jsonl) {
            json::print_json(&document)
        } else {
            println!("{}", crate::output::text::terminal_safe(&text));
            Ok(())
        }
    };
    match &cmd.command {
        ConfigSubcommand::Init => {
            let path = ensure_default_config()?.display().to_string();
            print(path.clone(), serde_json::json!({ "path": path }))
        }
        ConfigSubcommand::Get { key } => {
            let value = get_config_value(key)?
                .ok_or_else(|| IdxError::ConfigError(format!("key not found: {key}")))?;
            print(
                value.to_string(),
                serde_json::json!({ "key": key, "value": value.to_string() }),
            )
        }
        ConfigSubcommand::Set { key, value } => {
            set_config_value(key, value)?;
            print(
                "ok".to_string(),
                serde_json::json!({ "key": key, "value": value, "ok": true }),
            )
        }
        ConfigSubcommand::Path => {
            let path = config_path()?.display().to_string();
            print(path.clone(), serde_json::json!({ "path": path }))
        }
    }
}
