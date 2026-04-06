use chrono::NaiveDate;
use rusqlite::{Connection, params};

use crate::error::IdxError;
use crate::ownership::types::{
    InvestorTypeCode, KseiHoldingDraft, KseiRawRow, Locality, OwnershipSource,
};

/// Parse Indonesian locale number string to i64.
/// "1.533.682.440" → 1533682440, "0" → 0
pub fn parse_id_number(s: &str) -> Result<i64, IdxError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(IdxError::ParseError(
            "empty Indonesian number string".to_string(),
        ));
    }

    let normalized = trimmed.replace(['.', ' '], "");
    if normalized.is_empty() || !normalized.chars().all(|c| c.is_ascii_digit()) {
        return Err(IdxError::ParseError(format!(
            "invalid Indonesian number format: {s}"
        )));
    }

    normalized
        .parse::<i64>()
        .map_err(|e| IdxError::ParseError(format!("failed to parse Indonesian number '{s}': {e}")))
}

fn parse_id_number_or_zero(s: &str) -> Result<i64, IdxError> {
    if s.trim().is_empty() {
        Ok(0)
    } else {
        parse_id_number(s)
    }
}

/// Parse Indonesian locale percentage to basis points (i64).
/// "54,94" → 5494, "0,00" → 0, "100,00" → 10000
pub fn parse_id_percentage(s: &str) -> Result<i64, IdxError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(IdxError::ParseError(
            "empty Indonesian percentage string".to_string(),
        ));
    }

    let mut parts = trimmed.split(',');
    let whole = parts.next().unwrap_or_default();
    let frac = parts.next().unwrap_or("00");

    if parts.next().is_some() {
        return Err(IdxError::ParseError(format!(
            "invalid Indonesian percentage format: {s}"
        )));
    }

    if whole.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) {
        return Err(IdxError::ParseError(format!(
            "invalid Indonesian percentage whole part: {s}"
        )));
    }

    if !frac.chars().all(|c| c.is_ascii_digit()) || frac.len() > 2 {
        return Err(IdxError::ParseError(format!(
            "invalid Indonesian percentage fractional part: {s}"
        )));
    }

    let whole_i = whole.parse::<i64>().map_err(|e| {
        IdxError::ParseError(format!("failed to parse Indonesian percentage '{s}': {e}"))
    })?;

    let frac_padded = if frac.len() == 1 {
        format!("{frac}0")
    } else {
        frac.to_string()
    };
    let frac_i = frac_padded.parse::<i64>().map_err(|e| {
        IdxError::ParseError(format!("failed to parse Indonesian percentage '{s}': {e}"))
    })?;

    Ok((whole_i * 100) + frac_i)
}

/// Parse KSEI date format to NaiveDate.
/// "27-Feb-2026" → NaiveDate(2026, 2, 27)
pub fn parse_ksei_date(s: &str) -> Result<NaiveDate, IdxError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(IdxError::ParseError("empty KSEI date string".to_string()));
    }

    NaiveDate::parse_from_str(trimmed, "%d-%b-%Y")
        .map_err(|e| IdxError::ParseError(format!("invalid KSEI date '{s}': {e}")))
}

/// Normalize an investor name for entity matching.
/// Strips: "PT.", "PT ", "TBK", "Tbk", "(PERSERO)", "LIMITED", "PTE", "LTD"
/// Collapses whitespace, trims, uppercases.
pub fn normalize_name(raw: &str) -> String {
    let upper = collapse_whitespace(raw).to_uppercase();
    if upper.is_empty() {
        return upper;
    }

    let mut tokens: Vec<&str> = upper.split_whitespace().collect();

    loop {
        let mut changed = false;

        if let Some(first) = tokens.first().copied()
            && matches!(first, "PT" | "PT.")
        {
            tokens.remove(0);
            changed = true;
        }

        if let Some(last) = tokens.last().copied()
            && matches!(
                last,
                "TBK" | "Tbk" | "(PERSERO)" | "LIMITED" | "PTE" | "LTD"
            )
        {
            let _ = tokens.pop();
            changed = true;
        }

        if !changed {
            break;
        }
    }

    collapse_whitespace(&tokens.join(" "))
}

/// Convert a KseiRawRow into a normalized KseiHoldingDraft.
/// Applies all parsing functions above.
pub fn normalize_ksei_row(raw: &KseiRawRow) -> Result<KseiHoldingDraft, IdxError> {
    let investor_type = normalize_investor_type(&raw.investor_type);
    let locality = normalize_locality(&raw.local_foreign);

    Ok(KseiHoldingDraft {
        ticker_code: raw.share_code.trim().to_string(),
        issuer_name: optional_string(&raw.issuer_name),
        raw_investor_name: raw.investor_name.trim().to_string(),
        investor_type,
        locality,
        nationality: optional_string(&raw.nationality),
        domicile: optional_string(&raw.domicile),
        holdings_scripless: parse_id_number_or_zero(&raw.holdings_scripless)?,
        holdings_scrip: parse_id_number_or_zero(&raw.holdings_scrip)?,
        total_shares: parse_id_number(&raw.total_holding_shares)?,
        percentage_bps: parse_id_percentage(&raw.percentage)?,
        report_date: parse_ksei_date(&raw.date)?,
    })
}

/// Find or create a canonical entity for a raw investor name.
/// Strategy: exact match on normalized name → rule-based normalization → create new.
/// Returns entity_id.
pub fn resolve_entity(
    conn: &Connection,
    raw_name: &str,
    source: OwnershipSource,
) -> Result<i64, IdxError> {
    let source_db = source_to_db(source);
    let raw_trimmed = raw_name.trim();

    if raw_trimmed.is_empty() {
        return Err(IdxError::ParseError("empty raw entity name".to_string()));
    }

    let exact_normalized = collapse_whitespace(raw_trimmed).to_uppercase();
    let rule_normalized = normalize_name(raw_trimmed);

    if let Some(entity_id) = find_entity_by_alias(conn, raw_trimmed, source_db)? {
        return Ok(entity_id);
    }

    if let Some(entity_id) = find_entity_by_canonical(conn, &exact_normalized)? {
        insert_alias(conn, entity_id, raw_trimmed, source_db, "exact")?;
        return Ok(entity_id);
    }

    if rule_normalized != exact_normalized
        && let Some(entity_id) = find_entity_by_canonical(conn, &rule_normalized)?
    {
        insert_alias(conn, entity_id, raw_trimmed, source_db, "rule")?;
        return Ok(entity_id);
    }

    let now: i64 = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO entities (canonical_name, entity_type, country, created_at, updated_at)
         VALUES (?1, NULL, NULL, ?2, ?2)",
        params![
            if rule_normalized.is_empty() {
                &exact_normalized
            } else {
                &rule_normalized
            },
            now
        ],
    )
    .map_err(|e| IdxError::DatabaseError(format!("insert entity failed: {e}")))?;

    let entity_id = conn.last_insert_rowid();
    insert_alias(conn, entity_id, raw_trimmed, source_db, "exact")?;

    Ok(entity_id)
}

fn normalize_investor_type(raw: &str) -> Option<InvestorTypeCode> {
    let value = raw.trim();
    if value.is_empty() {
        None
    } else {
        Some(InvestorTypeCode(value.to_uppercase()))
    }
}

fn normalize_locality(raw: &str) -> Option<Locality> {
    match raw.trim().to_uppercase().as_str() {
        "L" | "D" => Some(Locality::Local),
        "F" | "A" => Some(Locality::Foreign),
        _ => None,
    }
}

fn optional_string(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn source_to_db(source: OwnershipSource) -> &'static str {
    match source {
        OwnershipSource::Ksei => "ksei",
        OwnershipSource::Bing => "bing",
    }
}

fn find_entity_by_alias(
    conn: &Connection,
    raw_name: &str,
    source: &str,
) -> Result<Option<i64>, IdxError> {
    conn.query_row(
        "SELECT entity_id FROM entity_aliases WHERE raw_name = ?1 AND source = ?2",
        params![raw_name, source],
        |row| row.get(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        _ => Err(IdxError::DatabaseError(e.to_string())),
    })
}

fn find_entity_by_canonical(
    conn: &Connection,
    canonical_name: &str,
) -> Result<Option<i64>, IdxError> {
    conn.query_row(
        "SELECT id FROM entities WHERE canonical_name = ?1",
        params![canonical_name],
        |row| row.get(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        _ => Err(IdxError::DatabaseError(e.to_string())),
    })
}

fn insert_alias(
    conn: &Connection,
    entity_id: i64,
    raw_name: &str,
    source: &str,
    method: &str,
) -> Result<(), IdxError> {
    conn.execute(
        "INSERT OR IGNORE INTO entity_aliases (entity_id, raw_name, source, confidence, method)
         VALUES (?1, ?2, ?3, 1.0, ?4)",
        params![entity_id, raw_name, source, method],
    )
    .map_err(|e| IdxError::DatabaseError(format!("insert alias failed: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rusqlite::Connection;

    use crate::ownership::entities::{
        normalize_ksei_row, normalize_name, parse_id_number, parse_id_percentage, parse_ksei_date,
        resolve_entity,
    };
    use crate::ownership::types::{KseiRawRow, OwnershipSource};

    #[test]
    fn test_parse_id_number() {
        assert_eq!(parse_id_number("1.533.682.440").unwrap(), 1_533_682_440);
        assert_eq!(parse_id_number("0").unwrap(), 0);
        assert_eq!(parse_id_number("3.200.142.830").unwrap(), 3_200_142_830);
        assert!(parse_id_number("").is_err());
    }

    #[test]
    fn test_parse_id_percentage() {
        assert_eq!(parse_id_percentage("54,94").unwrap(), 5494);
        assert_eq!(parse_id_percentage("0,00").unwrap(), 0);
        assert_eq!(parse_id_percentage("100,00").unwrap(), 10000);
        assert_eq!(parse_id_percentage("41,10").unwrap(), 4110);
    }

    #[test]
    fn test_parse_ksei_date_various_months() {
        let samples = [
            ("01-Jan-2026", NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            ("01-Feb-2026", NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
            ("01-Mar-2026", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()),
            ("01-Apr-2026", NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()),
            ("01-May-2026", NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
            ("01-Jun-2026", NaiveDate::from_ymd_opt(2026, 6, 1).unwrap()),
            ("01-Jul-2026", NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
            ("01-Aug-2026", NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()),
            ("01-Sep-2026", NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()),
            ("01-Oct-2026", NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()),
            ("01-Nov-2026", NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()),
            ("01-Dec-2026", NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()),
        ];

        for (input, expected) in samples {
            assert_eq!(parse_ksei_date(input).unwrap(), expected);
        }

        assert_eq!(
            parse_ksei_date("27-Feb-2026").unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 27).unwrap()
        );
    }

    #[test]
    fn test_normalize_name() {
        assert_eq!(
            normalize_name("PT. ASTRA INTERNATIONAL TBK"),
            "ASTRA INTERNATIONAL"
        );
        assert_eq!(
            normalize_name("PT BANK CENTRAL ASIA Tbk"),
            "BANK CENTRAL ASIA"
        );
        assert_eq!(
            normalize_name("UOB KAY HIAN PRIVATE LIMITED"),
            "UOB KAY HIAN PRIVATE"
        );
        assert_eq!(
            normalize_name("DJS Ketenagakerjaan (JHT)"),
            "DJS KETENAGAKERJAAN (JHT)"
        );
        assert_eq!(
            normalize_name("BPJS KETENAGAKERJAAN"),
            "BPJS KETENAGAKERJAAN"
        );
    }

    #[test]
    fn test_resolve_entity_create_then_reuse() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE entities (
                id INTEGER PRIMARY KEY,
                canonical_name TEXT NOT NULL,
                entity_type TEXT,
                country TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE entity_aliases (
                id INTEGER PRIMARY KEY,
                entity_id INTEGER NOT NULL,
                raw_name TEXT NOT NULL,
                source TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 1.0,
                method TEXT NOT NULL,
                UNIQUE(raw_name, source)
            );
            "#,
        )
        .unwrap();

        let id1 =
            resolve_entity(&conn, "PT. ASTRA INTERNATIONAL TBK", OwnershipSource::Ksei).unwrap();
        let id2 =
            resolve_entity(&conn, "PT. ASTRA INTERNATIONAL TBK", OwnershipSource::Ksei).unwrap();

        assert_eq!(id1, id2);
    }

    #[test]
    fn test_normalize_ksei_row_defaults_empty_component_shares_to_zero() {
        let raw = KseiRawRow {
            date: "27-Apr-2026".to_string(),
            share_code: "AADI".to_string(),
            issuer_name: "ADARO ANDALAN INDONESIA Tbk".to_string(),
            investor_name: "ADARO STRATEGIC INVESTMENTS".to_string(),
            investor_type: "CP".to_string(),
            local_foreign: "D".to_string(),
            nationality: "INDONESIA".to_string(),
            domicile: String::new(),
            holdings_scripless: String::new(),
            holdings_scrip: String::new(),
            total_holding_shares: "3.200.142.830".to_string(),
            percentage: "66,18".to_string(),
        };

        let draft = normalize_ksei_row(&raw).expect("empty component shares should normalize");
        assert_eq!(draft.holdings_scripless, 0);
        assert_eq!(draft.holdings_scrip, 0);
        assert_eq!(draft.total_shares, 3_200_142_830);
    }

    #[test]
    fn test_normalize_ksei_row_still_requires_total_shares() {
        let raw = KseiRawRow {
            date: "27-Apr-2026".to_string(),
            share_code: "AADI".to_string(),
            issuer_name: "ADARO ANDALAN INDONESIA Tbk".to_string(),
            investor_name: "ADARO STRATEGIC INVESTMENTS".to_string(),
            investor_type: "CP".to_string(),
            local_foreign: "D".to_string(),
            nationality: "INDONESIA".to_string(),
            domicile: String::new(),
            holdings_scripless: String::new(),
            holdings_scrip: String::new(),
            total_holding_shares: String::new(),
            percentage: "66,18".to_string(),
        };

        assert!(normalize_ksei_row(&raw).is_err());
    }
}
