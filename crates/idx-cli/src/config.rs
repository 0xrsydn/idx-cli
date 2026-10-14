use std::fs;
use std::io::IsTerminal;

use serde::Deserialize;

pub use idx_core::config::*;

use crate::cli::Cli;
use crate::output::{OutputFormat, OutputSetting};
use idx_core::error::IdxError;

/// CLI settings: core data-access settings plus presentation.
#[derive(Debug, Clone)]
pub struct IdxConfig {
    pub core: CoreConfig,
    pub output: OutputFormat,
    pub no_color: bool,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    general: Option<FileGeneral>,
}

#[derive(Debug, Deserialize, Default)]
struct FileGeneral {
    output: Option<OutputSetting>,
    color: Option<bool>,
}

impl IdxConfig {
    pub fn load_with_cli(cli: &Cli) -> Result<Self, IdxError> {
        let mut core = CoreConfig::load()?;
        core.verbose = cli.verbose > 0;
        let mut output = OutputSetting::Auto.resolve(stdout_is_terminal());
        let mut no_color = false;

        let path = config_path()?;
        if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|e| IdxError::Io(e.to_string()))?;
            let parsed: FileConfig =
                toml::from_str(&raw).map_err(|e| IdxError::ConfigError(e.to_string()))?;
            if let Some(general) = parsed.general {
                if let Some(setting) = general.output {
                    output = setting.resolve(stdout_is_terminal());
                }
                if let Some(color) = general.color {
                    no_color = !color;
                }
            }
        }

        if let Ok(value) = std::env::var("IDX_OUTPUT") {
            let setting = OutputSetting::parse(&value).ok_or_else(|| {
                IdxError::ConfigError(format!(
                    "invalid IDX_OUTPUT value: '{value}', expected 'auto', 'table', 'json', or 'jsonl'"
                ))
            })?;
            output = setting.resolve(stdout_is_terminal());
        }
        if let Ok(value) = std::env::var("IDX_NO_COLOR") {
            no_color = value == "1" || value.eq_ignore_ascii_case("true");
        }

        if let Some(setting) = cli.output {
            output = setting.resolve(stdout_is_terminal());
        }
        no_color = no_color || cli.no_color || !stdout_is_terminal();

        Ok(Self {
            core,
            output,
            no_color,
        })
    }
}

pub fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
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
