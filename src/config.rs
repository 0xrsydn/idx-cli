use std::fs;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::Deserialize;

use crate::cli::Cli;
use crate::error::IdxError;
use crate::output::OutputFormat;

#[derive(Debug, Clone)]
pub struct IdxConfig {
    pub exchange: String,
    pub output: OutputFormat,
    pub no_color: bool,
    pub quote_ttl: u64,
    pub fundamental_ttl: u64,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    general: Option<FileGeneral>,
    cache: Option<FileCache>,
}

#[derive(Debug, Deserialize, Default)]
struct FileGeneral {
    exchange: Option<String>,
    output: Option<OutputFormat>,
    color: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct FileCache {
    quote_ttl: Option<u64>,
    fundamental_ttl: Option<u64>,
}

impl Default for IdxConfig {
    fn default() -> Self {
        Self {
            exchange: "JK".to_string(),
            output: OutputFormat::Table,
            no_color: false,
            quote_ttl: 300,
            fundamental_ttl: 3600,
        }
    }
}

impl IdxConfig {
    pub fn load_with_cli(cli: &Cli) -> Result<Self, IdxError> {
        let mut cfg = Self::load()?;

        if let Ok(exchange) = std::env::var("IDX_EXCHANGE") {
            cfg.exchange = exchange;
        }
        if let Ok(output) = std::env::var("IDX_OUTPUT") {
            cfg.output = if output.eq_ignore_ascii_case("json") {
                OutputFormat::Json
            } else {
                OutputFormat::Table
            };
        }
        if let Ok(no_color) = std::env::var("IDX_NO_COLOR") {
            cfg.no_color = no_color == "1" || no_color.eq_ignore_ascii_case("true");
        }

        cfg.output = cli.output;
        cfg.no_color = cfg.no_color || cli.no_color;

        Ok(cfg)
    }

    pub fn load() -> Result<Self, IdxError> {
        let mut cfg = Self::default();
        let path = config_path()?;
        if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|e| IdxError::Io(e.to_string()))?;
            let parsed: FileConfig = toml::from_str(&raw).map_err(|e| IdxError::ConfigError(e.to_string()))?;
            if let Some(general) = parsed.general {
                if let Some(exchange) = general.exchange {
                    cfg.exchange = exchange;
                }
                if let Some(output) = general.output {
                    cfg.output = output;
                }
                if let Some(color) = general.color {
                    cfg.no_color = !color;
                }
            }
            if let Some(cache) = parsed.cache {
                if let Some(v) = cache.quote_ttl {
                    cfg.quote_ttl = v;
                }
                if let Some(v) = cache.fundamental_ttl {
                    cfg.fundamental_ttl = v;
                }
            }
        }
        Ok(cfg)
    }
}

pub fn config_path() -> Result<PathBuf, IdxError> {
    ProjectDirs::from("com", "idx", "idx")
        .map(|d| d.config_dir().join("config.toml"))
        .ok_or_else(|| IdxError::ConfigError("unable to resolve config dir".to_string()))
}

#[cfg(test)]
mod tests {
    use super::IdxConfig;

    #[test]
    fn default_values_are_sane() {
        let cfg = IdxConfig::default();
        assert_eq!(cfg.exchange, "JK");
        assert_eq!(cfg.quote_ttl, 300);
    }
}
