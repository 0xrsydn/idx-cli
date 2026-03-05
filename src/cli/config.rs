use clap::{Args, Subcommand};

use crate::config::{config_path, ensure_default_config, get_config_value, set_config_value};
use crate::error::IdxError;

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

pub fn handle(cmd: &ConfigCmd) -> Result<(), IdxError> {
    match &cmd.command {
        ConfigSubcommand::Init => {
            let path = ensure_default_config()?;
            println!("{}", path.display());
        }
        ConfigSubcommand::Get { key } => {
            let value = get_config_value(key)?
                .ok_or_else(|| IdxError::ConfigError(format!("key not found: {key}")))?;
            println!("{value}");
        }
        ConfigSubcommand::Set { key, value } => {
            set_config_value(key, value)?;
            println!("ok");
        }
        ConfigSubcommand::Path => {
            println!("{}", config_path()?.display());
        }
    }
    Ok(())
}
