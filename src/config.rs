use std::fs;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::Deserialize;

use crate::cli::Cli;
use crate::error::IdxError;
use crate::output::OutputFormat;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Yahoo,
    Msn,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yahoo => "yahoo",
            Self::Msn => "msn",
        }
    }

    fn parse(value: &str) -> Result<Self, IdxError> {
        if value.eq_ignore_ascii_case("yahoo") {
            Ok(Self::Yahoo)
        } else if value.eq_ignore_ascii_case("msn") {
            Ok(Self::Msn)
        } else {
            Err(IdxError::ConfigError(format!(
                "invalid provider '{value}' (expected yahoo or msn)"
            )))
        }
    }
}

#[derive(Debug, Clone)]
pub struct IdxConfig {
    pub provider: ProviderKind,
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
            provider: ProviderKind::Yahoo,
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

        if let Ok(provider) = std::env::var("IDX_PROVIDER") {
            cfg.provider = ProviderKind::parse(&provider)?;
        }
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
        if let Ok(v) = std::env::var("IDX_CACHE_QUOTE_TTL")
            && let Ok(parsed) = v.parse::<u64>()
        {
            cfg.quote_ttl = parsed;
        }
        if let Ok(v) = std::env::var("IDX_CACHE_FUNDAMENTAL_TTL")
            && let Ok(parsed) = v.parse::<u64>()
        {
            cfg.fundamental_ttl = parsed;
        }

        if let Some(output) = cli.output {
            cfg.output = output;
        }
        cfg.no_color = cfg.no_color || cli.no_color;

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

pub fn default_config_toml() -> String {
    "[general]\nprovider = \"yahoo\"\nexchange = \"JK\"\noutput = \"table\"\ncolor = true\n\n[cache]\nquote_ttl = 300\nfundamental_ttl = 3600\n".to_string()
}

pub fn config_path() -> Result<PathBuf, IdxError> {
    ProjectDirs::from("", "", "idx")
        .map(|d| d.config_dir().join("config.toml"))
        .ok_or_else(|| IdxError::ConfigError("unable to resolve config dir".to_string()))
}

pub fn ensure_default_config() -> Result<PathBuf, IdxError> {
    let path = config_path()?;
    if path.exists() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| IdxError::Io(e.to_string()))?;
    }
    fs::write(&path, default_config_toml()).map_err(|e| IdxError::Io(e.to_string()))?;
    Ok(path)
}

pub fn get_config_value(key: &str) -> Result<Option<String>, IdxError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(|e| IdxError::Io(e.to_string()))?;
    let value: toml::Value =
        toml::from_str(&raw).map_err(|e| IdxError::ConfigError(e.to_string()))?;

    let mut cur = &value;
    for part in key.split('.') {
        let Some(next) = cur.get(part) else {
            return Ok(None);
        };
        cur = next;
    }
    Ok(Some(cur.to_string().trim_matches('"').to_string()))
}

pub fn set_config_value(key: &str, value: &str) -> Result<(), IdxError> {
    let path = ensure_default_config()?;
    let raw = fs::read_to_string(&path).map_err(|e| IdxError::Io(e.to_string()))?;
    let mut root: toml::Value =
        toml::from_str(&raw).map_err(|e| IdxError::ConfigError(e.to_string()))?;

    let mut parts = key.split('.').peekable();
    let mut current = root
        .as_table_mut()
        .ok_or_else(|| IdxError::ConfigError("config root is not a table".to_string()))?;

    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            current.insert(part.to_string(), parse_toml_value(value));
        } else {
            let entry = current
                .entry(part.to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            if !entry.is_table() {
                *entry = toml::Value::Table(toml::map::Map::new());
            }
            current = entry
                .as_table_mut()
                .ok_or_else(|| IdxError::ConfigError("invalid config path".to_string()))?;
        }
    }

    fs::write(
        &path,
        toml::to_string_pretty(&root).map_err(|e| IdxError::ConfigError(e.to_string()))?,
    )
    .map_err(|e| IdxError::Io(e.to_string()))
}

fn parse_toml_value(value: &str) -> toml::Value {
    if let Ok(v) = value.parse::<i64>() {
        return toml::Value::Integer(v);
    }
    if let Ok(v) = value.parse::<f64>() {
        return toml::Value::Float(v);
    }
    if let Ok(v) = value.parse::<bool>() {
        return toml::Value::Boolean(v);
    }
    toml::Value::String(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::IdxConfig;

    #[test]
    fn default_values_are_sane() {
        let cfg = IdxConfig::default();
        assert_eq!(cfg.provider, super::ProviderKind::Yahoo);
        assert_eq!(cfg.exchange, "JK");
        assert_eq!(cfg.quote_ttl, 300);
    }

    #[test]
    fn parses_provider_values() {
        assert_eq!(
            super::ProviderKind::parse("yahoo").expect("yahoo provider"),
            super::ProviderKind::Yahoo
        );
        assert_eq!(
            super::ProviderKind::parse("MSN").expect("msn provider"),
            super::ProviderKind::Msn
        );
        assert!(super::ProviderKind::parse("unknown").is_err());
    }
}
