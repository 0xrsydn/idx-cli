use std::fs;
use std::io::IsTerminal;

use serde::Deserialize;

pub use idx_core::config::*;

use crate::cli::Cli;
use crate::output::{OutputFormat, OutputSetting};
use idx_core::error::IdxError;

#[derive(Debug, Clone)]
pub struct IdxConfig {
    pub provider: ProviderKind,
    pub history_provider: HistoryProviderKind,
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
    provider: Option<ProviderKind>,
    history_provider: Option<HistoryProviderKind>,
    exchange: Option<String>,
    output: Option<OutputSetting>,
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
            provider: ProviderKind::Msn,
            history_provider: HistoryProviderKind::Auto,
            exchange: "JK".to_string(),
            output: OutputSetting::Auto.resolve(stdout_is_terminal()),
            no_color: false,
            quote_ttl: 300,
            fundamental_ttl: 3600,
        }
    }
}

impl IdxConfig {
    pub fn load_with_cli(cli: &Cli) -> Result<Self, IdxError> {
        let mut cfg = Self::load()?;

        if let Ok(provider) = std::env::var("IDX_PROVIDER") {
            cfg.provider = ProviderKind::parse(&provider)?;
        }
        if let Ok(history_provider) = std::env::var("IDX_HISTORY_PROVIDER") {
            cfg.history_provider = HistoryProviderKind::parse(&history_provider)?;
        }
        if let Ok(exchange) = std::env::var("IDX_EXCHANGE") {
            cfg.exchange = exchange;
        }
        if let Ok(output) = std::env::var("IDX_OUTPUT") {
            let setting = OutputSetting::parse(&output).ok_or_else(|| {
                IdxError::ConfigError(format!(
                    "invalid IDX_OUTPUT value: '{output}', expected 'auto', 'table', 'json', or 'jsonl'"
                ))
            })?;
            cfg.output = setting.resolve(stdout_is_terminal());
        }
        if let Ok(no_color) = std::env::var("IDX_NO_COLOR") {
            cfg.no_color = no_color == "1" || no_color.eq_ignore_ascii_case("true");
        }
        if let Some(parsed) = parse_env_ttl("IDX_CACHE_QUOTE_TTL")? {
            cfg.quote_ttl = parsed;
        }
        if let Some(parsed) = parse_env_ttl("IDX_CACHE_FUNDAMENTAL_TTL")? {
            cfg.fundamental_ttl = parsed;
        }

        if let Some(output) = cli.output {
            cfg.output = output.resolve(stdout_is_terminal());
        }
        cfg.no_color = cfg.no_color || cli.no_color || !stdout_is_terminal();

        Ok(cfg)
    }

    pub fn load() -> Result<Self, IdxError> {
        let mut cfg = Self::default();
        let path = config_path()?;
        if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|e| IdxError::Io(e.to_string()))?;
            let parsed: FileConfig =
                toml::from_str(&raw).map_err(|e| IdxError::ConfigError(e.to_string()))?;
            if let Some(general) = parsed.general {
                if let Some(provider) = general.provider {
                    cfg.provider = provider;
                }
                if let Some(history_provider) = general.history_provider {
                    cfg.history_provider = history_provider;
                }
                if let Some(exchange) = general.exchange {
                    cfg.exchange = exchange;
                }
                if let Some(output) = general.output {
                    cfg.output = output.resolve(stdout_is_terminal());
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

pub fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::IdxConfig;

    #[test]
    fn default_values_are_sane() {
        let cfg = IdxConfig::default();
        assert_eq!(cfg.provider, super::ProviderKind::Msn);
        assert_eq!(cfg.history_provider, super::HistoryProviderKind::Auto);
        assert_eq!(cfg.exchange, "JK");
        assert_eq!(cfg.quote_ttl, 300);
    }
}

#[cfg(test)]
mod output_values_tests {
    use clap::ValueEnum;

    use crate::output::OutputSetting;

    #[test]
    fn core_output_values_match_cli_output_setting() {
        let cli: Vec<String> = OutputSetting::value_variants()
            .iter()
            .filter_map(|v| v.to_possible_value())
            .map(|v| v.get_name().to_string())
            .collect();
        assert_eq!(cli, super::OUTPUT_VALUES);
    }
}
