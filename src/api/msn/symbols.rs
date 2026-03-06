use std::collections::HashMap;
use std::sync::OnceLock;

const SYMBOL_IDS_RAW: &str = include_str!("symbol_ids.tsv");

static SYMBOL_IDS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

fn symbol_ids() -> &'static HashMap<&'static str, &'static str> {
    SYMBOL_IDS.get_or_init(|| {
        SYMBOL_IDS_RAW
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .collect()
    })
}

pub(crate) fn ticker_from_symbol(symbol: &str) -> Option<String> {
    let trimmed = symbol.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .split('.')
            .next()
            .unwrap_or(trimmed)
            .trim()
            .to_uppercase(),
    )
}

pub(crate) fn resolve_msn_id(symbol: &str) -> Option<&'static str> {
    let ticker = ticker_from_symbol(symbol)?;
    symbol_ids().get(ticker.as_str()).copied()
}

pub(crate) fn normalized_symbol(requested: &str, fallback_ticker: &str) -> String {
    let trimmed = requested.trim().to_uppercase();
    if trimmed.contains('.') || fallback_ticker.is_empty() {
        trimmed
    } else {
        format!("{}.JK", fallback_ticker.trim().to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_msn_id;

    #[test]
    fn resolves_symbol_variants() {
        assert_eq!(resolve_msn_id("BBCA"), Some("bn91jc"));
        assert_eq!(resolve_msn_id("bbca.jk"), Some("bn91jc"));
        assert_eq!(resolve_msn_id("INVALID"), None);
    }
}
