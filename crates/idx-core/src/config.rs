//! Config file location, provider selection, and `config.toml` read/write.
//! Presentation settings (output format, color) are resolved by frontends.

use std::fs;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::Deserialize;

use crate::error::IdxError;

/// Values accepted for `general.output`; frontends decide what they mean.
pub const OUTPUT_VALUES: &[&str] = &["auto", "table", "json", "jsonl"];

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Yahoo,
    Msn,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "clap", value(rename_all = "lower"))]
pub enum HistoryProviderKind {
    Auto,
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

    pub fn parse(value: &str) -> Result<Self, IdxError> {
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

impl HistoryProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Yahoo => "yahoo",
            Self::Msn => "msn",
        }
    }

    pub fn parse(value: &str) -> Result<Self, IdxError> {
        if value.eq_ignore_ascii_case("auto") {
            Ok(Self::Auto)
        } else if value.eq_ignore_ascii_case("yahoo") {
            Ok(Self::Yahoo)
        } else if value.eq_ignore_ascii_case("msn") {
            Ok(Self::Msn)
        } else {
            Err(IdxError::ConfigError(format!(
                "invalid history provider '{value}' (expected auto, yahoo, or msn)"
            )))
        }
    }
}

pub fn parse_env_ttl(name: &str) -> Result<Option<u64>, IdxError> {
    let Ok(raw) = std::env::var(name) else {
        return Ok(None);
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(IdxError::ConfigError(format!(
            "invalid {name} value '{raw}': expected a non-negative integer"
        )));
    }

    let parsed = trimmed.parse::<u64>().map_err(|_| {
        IdxError::ConfigError(format!(
            "invalid {name} value '{raw}': expected a non-negative integer"
        ))
    })?;

    Ok(Some(parsed))
}

pub fn default_config_toml() -> String {
    "[general]\nprovider = \"msn\"\nhistory_provider = \"auto\"\nexchange = \"JK\"\noutput = \"auto\"\ncolor = true\n\n[cache]\nquote_ttl = 300\nfundamental_ttl = 3600\n".to_string()
}

pub fn config_path() -> Result<PathBuf, IdxError> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME")
        && !dir.is_empty()
    {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return Ok(path.join("idx").join("config.toml"));
        }
    }
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

/// Known configuration keys and their valid values
const KNOWN_CONFIG_KEYS: &[&str] = &[
    "general.provider",
    "general.history_provider",
    "general.exchange",
    "general.output",
    "general.color",
    "cache.quote_ttl",
    "cache.fundamental_ttl",
    "ownership.db_path",
    "ownership.snapshot_manifest",
];

/// Validates a config key and value before writing
fn validate_config_key_value(key: &str, value: &str) -> Result<(), IdxError> {
    normalize_config_value(key, value).map(|_| ())
}

fn normalize_config_value(key: &str, value: &str) -> Result<toml::Value, IdxError> {
    match key {
        "general.provider" => Ok(toml::Value::String(
            ProviderKind::parse(value)?.as_str().to_string(),
        )),
        "general.history_provider" => Ok(toml::Value::String(
            HistoryProviderKind::parse(value)?.as_str().to_string(),
        )),
        "general.output" => {
            if !OUTPUT_VALUES.iter().any(|v| v.eq_ignore_ascii_case(value)) {
                return Err(IdxError::InvalidInput(format!(
                    "invalid output format '{value}': expected 'auto', 'table', 'json', or 'jsonl'"
                )));
            }
            Ok(toml::Value::String(value.to_ascii_lowercase()))
        }
        "cache.quote_ttl" | "cache.fundamental_ttl" => {
            let parsed: i64 = value.parse().map_err(|_| {
                IdxError::InvalidInput(format!(
                    "invalid TTL value '{value}': must be a non-negative integer"
                ))
            })?;
            if parsed < 0 {
                return Err(IdxError::InvalidInput(format!(
                    "invalid TTL value '{value}': must be non-negative"
                )));
            }
            Ok(toml::Value::Integer(parsed))
        }
        "general.exchange" => {
            // Exchange is free-form (e.g. JK, US, etc.)
            Ok(toml::Value::String(value.to_string()))
        }
        "ownership.db_path" => {
            // Ownership DB path is free-form and may be absolute or relative.
            Ok(toml::Value::String(value.to_string()))
        }
        "ownership.snapshot_manifest" => {
            // Snapshot manifest can be an absolute/relative path or a URL.
            Ok(toml::Value::String(value.to_string()))
        }
        "general.color" => {
            if !value.eq_ignore_ascii_case("true") && !value.eq_ignore_ascii_case("false") {
                return Err(IdxError::InvalidInput(format!(
                    "invalid color value '{value}': expected 'true' or 'false'"
                )));
            }
            Ok(toml::Value::Boolean(value.eq_ignore_ascii_case("true")))
        }
        _ => Err(IdxError::InvalidInput(format!(
            "unknown config key '{}'. Valid keys: {}",
            key,
            KNOWN_CONFIG_KEYS.join(", ")
        ))),
    }
}

pub fn set_config_value(key: &str, value: &str) -> Result<(), IdxError> {
    validate_config_key_value(key, value)?;
    let normalized = normalize_config_value(key, value)?;

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
            current.insert(part.to_string(), normalized.clone());
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

#[cfg(test)]
mod tests {
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

    #[test]
    fn parses_history_provider_values() {
        assert_eq!(
            super::HistoryProviderKind::parse("auto").expect("auto history provider"),
            super::HistoryProviderKind::Auto
        );
        assert_eq!(
            super::HistoryProviderKind::parse("yahoo").expect("yahoo history provider"),
            super::HistoryProviderKind::Yahoo
        );
        assert_eq!(
            super::HistoryProviderKind::parse("MSN").expect("msn history provider"),
            super::HistoryProviderKind::Msn
        );
        assert!(super::HistoryProviderKind::parse("unknown").is_err());
    }

    #[test]
    fn validate_rejects_unknown_keys() {
        let result = super::validate_config_key_value("unknown.key", "value");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, super::IdxError::InvalidInput(_)));
        assert!(err.to_string().contains("unknown config key"));
    }

    #[test]
    fn validate_rejects_invalid_provider() {
        let result = super::validate_config_key_value("general.provider", "bogus");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid provider"));
    }

    #[test]
    fn validate_accepts_valid_provider_yahoo() {
        assert!(super::validate_config_key_value("general.provider", "yahoo").is_ok());
    }

    #[test]
    fn validate_accepts_valid_provider_msn() {
        assert!(super::validate_config_key_value("general.provider", "msn").is_ok());
    }

    #[test]
    fn validate_accepts_valid_provider_case_insensitive() {
        assert!(super::validate_config_key_value("general.provider", "YAHOO").is_ok());
        assert!(super::validate_config_key_value("general.provider", "Msn").is_ok());
    }

    #[test]
    fn normalize_provider_lowercases_value() {
        assert_eq!(
            super::normalize_config_value("general.provider", "Msn").unwrap(),
            toml::Value::String("msn".to_string())
        );
    }

    #[test]
    fn validate_rejects_invalid_history_provider() {
        let result = super::validate_config_key_value("general.history_provider", "bogus");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid history provider")
        );
    }

    #[test]
    fn validate_accepts_valid_history_provider_auto() {
        assert!(super::validate_config_key_value("general.history_provider", "auto").is_ok());
    }

    #[test]
    fn validate_accepts_valid_history_provider_yahoo() {
        assert!(super::validate_config_key_value("general.history_provider", "yahoo").is_ok());
    }

    #[test]
    fn validate_accepts_valid_history_provider_msn() {
        assert!(super::validate_config_key_value("general.history_provider", "msn").is_ok());
    }

    #[test]
    fn normalize_history_provider_lowercases_value() {
        assert_eq!(
            super::normalize_config_value("general.history_provider", "Auto").unwrap(),
            toml::Value::String("auto".to_string())
        );
    }

    #[test]
    fn validate_rejects_invalid_output() {
        let result = super::validate_config_key_value("general.output", "bogus");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid output format")
        );
    }

    #[test]
    fn validate_accepts_valid_output_table() {
        assert!(super::validate_config_key_value("general.output", "table").is_ok());
    }

    #[test]
    fn validate_accepts_valid_output_json() {
        assert!(super::validate_config_key_value("general.output", "json").is_ok());
    }

    #[test]
    fn validate_accepts_valid_output_case_insensitive() {
        assert!(super::validate_config_key_value("general.output", "TABLE").is_ok());
        assert!(super::validate_config_key_value("general.output", "Json").is_ok());
    }

    #[test]
    fn normalize_output_lowercases_value() {
        assert_eq!(
            super::normalize_config_value("general.output", "Json").unwrap(),
            toml::Value::String("json".to_string())
        );
    }

    #[test]
    fn validate_rejects_negative_ttl() {
        let result = super::validate_config_key_value("cache.quote_ttl", "-1");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be non-negative")
        );
    }

    #[test]
    fn validate_rejects_non_numeric_ttl() {
        let result = super::validate_config_key_value("cache.quote_ttl", "not-a-number");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be a non-negative integer")
        );
    }

    #[test]
    fn validate_accepts_valid_ttl() {
        assert!(super::validate_config_key_value("cache.quote_ttl", "300").is_ok());
        assert!(super::validate_config_key_value("cache.fundamental_ttl", "3600").is_ok());
    }

    #[test]
    fn validate_accepts_zero_ttl() {
        assert!(super::validate_config_key_value("cache.quote_ttl", "0").is_ok());
    }

    #[test]
    fn validate_accepts_exchange_any_value() {
        assert!(super::validate_config_key_value("general.exchange", "JK").is_ok());
        assert!(super::validate_config_key_value("general.exchange", "US").is_ok());
    }

    #[test]
    fn validate_accepts_ownership_db_path() {
        assert!(super::validate_config_key_value("ownership.db_path", "/tmp/ownership.db").is_ok());
        assert!(super::validate_config_key_value("ownership.db_path", "data/ownership.db").is_ok());
    }

    #[test]
    fn validate_accepts_ownership_snapshot_manifest() {
        assert!(
            super::validate_config_key_value(
                "ownership.snapshot_manifest",
                "https://example.com/latest.json"
            )
            .is_ok()
        );
        assert!(
            super::validate_config_key_value(
                "ownership.snapshot_manifest",
                "snapshots/latest.json"
            )
            .is_ok()
        );
    }

    #[test]
    fn validate_accepts_valid_color() {
        assert!(super::validate_config_key_value("general.color", "true").is_ok());
        assert!(super::validate_config_key_value("general.color", "false").is_ok());
        assert!(super::validate_config_key_value("general.color", "TRUE").is_ok());
        assert!(super::validate_config_key_value("general.color", "False").is_ok());
    }

    #[test]
    fn normalize_color_writes_boolean() {
        assert_eq!(
            super::normalize_config_value("general.color", "True").unwrap(),
            toml::Value::Boolean(true)
        );
    }

    #[test]
    fn validate_rejects_invalid_color() {
        let result = super::validate_config_key_value("general.color", "foo");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid color value")
        );

        let result = super::validate_config_key_value("general.color", "123");
        assert!(result.is_err());
    }
}
