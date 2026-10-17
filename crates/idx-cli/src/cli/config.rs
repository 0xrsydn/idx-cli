use clap::{Args, Subcommand};

use crate::config::{config_path, ensure_default_config, get_config_value, set_config_value};
use crate::output::{OutputFormat, json};
use idx_core::error::IdxError;

#[derive(Debug, Args)]
#[command(about = "Manage configuration")]
pub struct ConfigCmd {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    #[command(about = "Create default configuration file")]
    Init,
    #[command(about = "Get a configuration value by key")]
    Get { key: String },
    #[command(about = "Set a configuration value by key")]
    Set { key: String, value: String },
    #[command(about = "Print configuration file path")]
    Path,
}

pub fn handle(cmd: &ConfigCmd, format: &OutputFormat) -> Result<(), IdxError> {
    // Each result as plain text, or one JSON document with -o json.
    let print = |text: String, document: serde_json::Value| {
        if format.is_json() {
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
