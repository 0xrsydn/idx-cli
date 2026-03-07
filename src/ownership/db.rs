use std::fs;
use std::path::PathBuf;

use chrono::NaiveDate;
use directories::ProjectDirs;
use rusqlite::{Connection, params};

use crate::config::{IdxConfig, get_config_value};
use crate::error::IdxError;
use crate::ownership::search;
use crate::ownership::types::{
    BingHolding, ChangeRow, ChangeType, ConcentrationMetrics, CrossHolderRow, Entity,
    EntityHoldings, EntityTickerRow, FlowSignal, HolderRow, InstitutionalFlow, KseiHolding,
    Locality, OwnershipRelease, OwnershipSource, Ticker, TickerOwnership, UnresolvedRow,
};

/// Ownership schema version 1 DDL.
pub const SCHEMA_V1: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS ownership_releases (
    id          INTEGER PRIMARY KEY,
    source_url  TEXT,
    sha256      TEXT NOT NULL UNIQUE,
    as_of_date  TEXT NOT NULL,
    row_count   INTEGER NOT NULL,
    imported_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS entities (
    id              INTEGER PRIMARY KEY,
    canonical_name  TEXT NOT NULL,
    entity_type     TEXT,
    country         TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS entity_fts USING fts5(
    canonical_name,
    aliases,
    content='entities',
    content_rowid='id',
    tokenize='trigram'
);

CREATE TABLE IF NOT EXISTS entity_aliases (
    id          INTEGER PRIMARY KEY,
    entity_id   INTEGER NOT NULL REFERENCES entities(id),
    raw_name    TEXT NOT NULL,
    source      TEXT NOT NULL,
    confidence  REAL NOT NULL DEFAULT 1.0,
    method      TEXT NOT NULL,
    UNIQUE(raw_name, source)
);
CREATE INDEX IF NOT EXISTS idx_aliases_entity ON entity_aliases(entity_id);
CREATE INDEX IF NOT EXISTS idx_aliases_name ON entity_aliases(raw_name);

CREATE TABLE IF NOT EXISTS tickers (
    id      INTEGER PRIMARY KEY,
    code    TEXT NOT NULL UNIQUE,
    name    TEXT,
    sector  TEXT
);

CREATE TABLE IF NOT EXISTS ksei_holdings (
    id                  INTEGER PRIMARY KEY,
    ticker_id           INTEGER NOT NULL REFERENCES tickers(id),
    entity_id           INTEGER REFERENCES entities(id),
    raw_investor_name   TEXT NOT NULL,
    investor_type       TEXT,
    locality            TEXT,
    nationality         TEXT,
    domicile            TEXT,
    holdings_scripless  INTEGER NOT NULL,
    holdings_scrip      INTEGER NOT NULL,
    total_shares        INTEGER NOT NULL,
    percentage_bps      INTEGER NOT NULL,
    report_date         TEXT NOT NULL,
    release_sha256      TEXT NOT NULL,
    UNIQUE(release_sha256, ticker_id, raw_investor_name)
);
CREATE INDEX IF NOT EXISTS idx_ksei_ticker ON ksei_holdings(ticker_id);
CREATE INDEX IF NOT EXISTS idx_ksei_entity ON ksei_holdings(entity_id);
CREATE INDEX IF NOT EXISTS idx_ksei_date ON ksei_holdings(report_date);
CREATE INDEX IF NOT EXISTS idx_ksei_pct ON ksei_holdings(percentage_bps DESC);

CREATE TABLE IF NOT EXISTS bing_holdings (
    id                  INTEGER PRIMARY KEY,
    ticker_id           INTEGER NOT NULL REFERENCES tickers(id),
    entity_id           INTEGER REFERENCES entities(id),
    raw_investor_name   TEXT NOT NULL,
    investor_type       TEXT,
    shares_held         INTEGER,
    shares_changed      INTEGER,
    pct_ownership_bps   INTEGER,
    value_usd           INTEGER,
    report_date         TEXT NOT NULL,
    signal              TEXT NOT NULL,
    fetched_at          INTEGER NOT NULL,
    UNIQUE(ticker_id, raw_investor_name, report_date, signal)
);
CREATE INDEX IF NOT EXISTS idx_bing_ticker ON bing_holdings(ticker_id);
CREATE INDEX IF NOT EXISTS idx_bing_entity ON bing_holdings(entity_id);
CREATE INDEX IF NOT EXISTS idx_bing_date ON bing_holdings(report_date);
"#;

/// Ensure ownership schema is migrated and ready for use.
pub fn ensure_schema(conn: &Connection) -> Result<(), IdxError> {
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    if conn.path().is_some() {
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
    }
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    if user_version < 1 {
        conn.execute_batch(SCHEMA_V1)
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
        conn.pragma_update(None, "user_version", 1)
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
    }

    Ok(())
}

/// Open ownership database connection and run idempotent schema migration.
pub fn open_db(_config: &IdxConfig) -> Result<Connection, IdxError> {
    let db_path = resolve_db_path()?;

    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).map_err(|e| IdxError::DatabaseError(e.to_string()))?;
    }

    let conn = Connection::open(&db_path).map_err(|e| IdxError::DatabaseError(e.to_string()))?;
    ensure_schema(&conn)?;

    Ok(conn)
}

/// Insert or get a ticker by code. Returns ticker_id.
pub fn upsert_ticker(conn: &Connection, code: &str, name: Option<&str>) -> Result<i64, IdxError> {
    conn.execute(
        "INSERT INTO tickers (code, name) VALUES (?1, ?2)
         ON CONFLICT(code) DO UPDATE SET
            name = COALESCE(excluded.name, tickers.name)",
        params![code, name],
    )
    .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    conn.query_row(
        "SELECT id FROM tickers WHERE code = ?1",
        params![code],
        |row| row.get(0),
    )
    .map_err(|e| IdxError::DatabaseError(e.to_string()))
}

/// Bulk insert KSEI holdings within a transaction.
/// Uses INSERT OR IGNORE for dedup (unique on release_sha256 + ticker_id + raw_investor_name).
pub fn insert_ksei_holdings(
    conn: &Connection,
    holdings: &[KseiHolding],
) -> Result<usize, IdxError> {
    if holdings.is_empty() {
        return Ok(0);
    }

    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut inserted = 0usize;
    for h in holdings {
        let locality = h.locality.map(locality_to_db);
        let investor_type = h.investor_type.as_ref().map(|v| v.0.as_str());
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO ksei_holdings (
                    ticker_id, entity_id, raw_investor_name, investor_type, locality,
                    nationality, domicile, holdings_scripless, holdings_scrip, total_shares,
                    percentage_bps, report_date, release_sha256
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    h.ticker_id,
                    h.entity_id,
                    h.raw_investor_name,
                    investor_type,
                    locality,
                    h.nationality,
                    h.domicile,
                    h.holdings_scripless,
                    h.holdings_scrip,
                    h.total_shares,
                    h.percentage_bps,
                    h.report_date.format("%Y-%m-%d").to_string(),
                    h.release_sha256,
                ],
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()));

        match changed {
            Ok(n) => inserted += n,
            Err(err) => {
                let _ = conn.execute("ROLLBACK", []);
                return Err(err);
            }
        }
    }

    conn.execute("COMMIT", [])
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    Ok(inserted)
}

/// Bulk insert Bing holdings within a transaction.
pub fn insert_bing_holdings(
    conn: &Connection,
    holdings: &[BingHolding],
) -> Result<usize, IdxError> {
    if holdings.is_empty() {
        return Ok(0);
    }

    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut inserted = 0usize;
    for h in holdings {
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO bing_holdings (
                    ticker_id, entity_id, raw_investor_name, investor_type, shares_held,
                    shares_changed, pct_ownership_bps, value_usd, report_date, signal, fetched_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    h.ticker_id,
                    h.entity_id,
                    h.raw_investor_name,
                    h.investor_type,
                    h.shares_held,
                    h.shares_changed,
                    h.pct_ownership_bps,
                    h.value_usd,
                    h.report_date.format("%Y-%m-%d").to_string(),
                    flow_signal_to_db(h.signal),
                    h.fetched_at,
                ],
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()));

        match changed {
            Ok(n) => inserted += n,
            Err(err) => {
                let _ = conn.execute("ROLLBACK", []);
                return Err(err);
            }
        }
    }

    conn.execute("COMMIT", [])
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    Ok(inserted)
}

/// Record a KSEI release import. Returns release_id.
pub fn insert_release(conn: &Connection, release: &OwnershipRelease) -> Result<i64, IdxError> {
    conn.execute(
        "INSERT INTO ownership_releases (source_url, sha256, as_of_date, row_count, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            release.source_url,
            release.sha256,
            release.as_of_date.format("%Y-%m-%d").to_string(),
            i64::try_from(release.row_count)
                .map_err(|e| IdxError::DatabaseError(format!("invalid row_count: {e}")))?,
            release.imported_at,
        ],
    )
    .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    Ok(conn.last_insert_rowid())
}

/// Check if a release with this SHA-256 already exists.
pub fn release_exists(conn: &Connection, sha256: &str) -> Result<bool, IdxError> {
    let exists: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM ownership_releases WHERE sha256 = ?1)",
            params![sha256],
            |row| row.get(0),
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    Ok(exists == 1)
}

/// Get combined KSEI + Bing holdings for a ticker.
/// Returns TickerOwnership with merged holders sorted by percentage desc,
/// concentration metrics, and Bing flow data.
pub fn query_ticker_holdings(conn: &Connection, code: &str) -> Result<TickerOwnership, IdxError> {
    let ticker =
        query_ticker(conn, code)?.ok_or_else(|| IdxError::SymbolNotFound(code.to_string()))?;

    let ksei_as_of = conn
        .query_row(
            "SELECT MAX(report_date) FROM ksei_holdings WHERE ticker_id = ?1",
            params![ticker.id],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?
        .map(|s| parse_iso_date(&s))
        .transpose()?;

    let bing_as_of = conn
        .query_row(
            "SELECT MAX(report_date) FROM bing_holdings WHERE ticker_id = ?1",
            params![ticker.id],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut holders: Vec<HolderRow> = Vec::new();

    {
        let mut stmt = conn
            .prepare(
                "SELECT k.entity_id, COALESCE(e.canonical_name, k.raw_investor_name),
                        k.investor_type, k.locality, k.total_shares, k.percentage_bps
                 FROM ksei_holdings k
                 LEFT JOIN entities e ON e.id = k.entity_id
                 WHERE k.ticker_id = ?1
                 ORDER BY k.percentage_bps DESC, k.total_shares DESC",
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map(params![ticker.id], |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        for row in rows {
            let (entity_id, name, investor_type, locality_raw, shares, percentage_bps) =
                row.map_err(|e| IdxError::DatabaseError(e.to_string()))?;
            holders.push(HolderRow {
                rank: 0,
                source: OwnershipSource::Ksei,
                name,
                entity_id,
                investor_type,
                locality: locality_raw.as_deref().and_then(locality_from_db),
                shares,
                percentage_bps,
                signal: None,
            });
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT b.entity_id, COALESCE(e.canonical_name, b.raw_investor_name),
                        b.investor_type, COALESCE(b.shares_held, 0), COALESCE(b.pct_ownership_bps, 0), b.signal
                 FROM bing_holdings b
                 LEFT JOIN entities e ON e.id = b.entity_id
                 WHERE b.ticker_id = ?1
                 ORDER BY COALESCE(b.pct_ownership_bps, 0) DESC, COALESCE(b.shares_held, 0) DESC",
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map(params![ticker.id], |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        for row in rows {
            let (entity_id, name, investor_type, shares, percentage_bps, signal_raw) =
                row.map_err(|e| IdxError::DatabaseError(e.to_string()))?;
            holders.push(HolderRow {
                rank: 0,
                source: OwnershipSource::Bing,
                name,
                entity_id,
                investor_type,
                locality: None,
                shares,
                percentage_bps,
                signal: flow_signal_from_db(&signal_raw),
            });
        }
    }

    holders.sort_by(|a, b| {
        b.percentage_bps
            .cmp(&a.percentage_bps)
            .then_with(|| b.shares.cmp(&a.shares))
    });
    for (idx, holder) in holders.iter_mut().enumerate() {
        holder.rank = idx + 1;
    }

    let percentages = holders.iter().map(|h| h.percentage_bps).collect::<Vec<_>>();
    let concentration = compute_concentration(&percentages);
    let flow = query_bing_flow(conn, ticker.id)?;

    Ok(TickerOwnership {
        ticker,
        ksei_as_of,
        bing_as_of,
        holders,
        concentration,
        flow,
    })
}

/// Get all holdings for an entity across tickers.
pub fn query_entity_holdings(
    conn: &Connection,
    entity_id: i64,
) -> Result<EntityHoldings, IdxError> {
    let entity = query_entity(conn, entity_id)?
        .ok_or_else(|| IdxError::SymbolNotFound(format!("entity:{entity_id}")))?;

    let mut holdings = Vec::new();

    {
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.code, t.name, t.sector, k.total_shares, k.percentage_bps, k.report_date
                 FROM ksei_holdings k
                 JOIN tickers t ON t.id = k.ticker_id
                 WHERE k.entity_id = ?1",
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map(params![entity_id], |row| {
                Ok(EntityTickerRow {
                    ticker: Ticker {
                        id: row.get(0)?,
                        code: row.get(1)?,
                        name: row.get(2)?,
                        sector: row.get(3)?,
                    },
                    source: OwnershipSource::Ksei,
                    shares: row.get(4)?,
                    percentage_bps: row.get(5)?,
                    report_date: parse_iso_date(&row.get::<_, String>(6)?)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                })
            })
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        for row in rows {
            holdings.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.code, t.name, t.sector,
                        COALESCE(b.shares_held, 0), COALESCE(b.pct_ownership_bps, 0), b.report_date
                 FROM bing_holdings b
                 JOIN tickers t ON t.id = b.ticker_id
                 WHERE b.entity_id = ?1",
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map(params![entity_id], |row| {
                Ok(EntityTickerRow {
                    ticker: Ticker {
                        id: row.get(0)?,
                        code: row.get(1)?,
                        name: row.get(2)?,
                        sector: row.get(3)?,
                    },
                    source: OwnershipSource::Bing,
                    shares: row.get(4)?,
                    percentage_bps: row.get(5)?,
                    report_date: parse_iso_date(&row.get::<_, String>(6)?)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                })
            })
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        for row in rows {
            holdings.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
        }
    }

    holdings.sort_by(|a, b| {
        b.percentage_bps
            .cmp(&a.percentage_bps)
            .then_with(|| a.ticker.code.cmp(&b.ticker.code))
    });

    let ticker_count = holdings
        .iter()
        .map(|h| h.ticker.id)
        .collect::<std::collections::HashSet<_>>()
        .len();

    Ok(EntityHoldings {
        entity,
        ticker_count,
        holdings,
    })
}

/// Rank entities by number of tickers they hold (cross-ownership breadth).
pub fn query_cross_holders(
    conn: &Connection,
    min_tickers: usize,
    limit: usize,
) -> Result<Vec<CrossHolderRow>, IdxError> {
    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.canonical_name, e.entity_type, e.country,
                    COUNT(DISTINCT u.ticker_id) AS ticker_count,
                    SUM(u.percentage_bps) AS total_bps
             FROM entities e
             JOIN (
                 SELECT entity_id, ticker_id, percentage_bps
                 FROM ksei_holdings
                 WHERE entity_id IS NOT NULL
                 UNION ALL
                 SELECT entity_id, ticker_id, COALESCE(pct_ownership_bps, 0) AS percentage_bps
                 FROM bing_holdings
                 WHERE entity_id IS NOT NULL
             ) u ON u.entity_id = e.id
             GROUP BY e.id
             HAVING COUNT(DISTINCT u.ticker_id) >= ?1
             ORDER BY ticker_count DESC, total_bps DESC, e.canonical_name ASC
             LIMIT ?2",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map(
            params![
                i64::try_from(min_tickers)
                    .map_err(|e| IdxError::DatabaseError(format!("invalid min_tickers: {e}")))?,
                i64::try_from(limit)
                    .map_err(|e| IdxError::DatabaseError(format!("invalid limit: {e}")))?,
            ],
            |row| {
                Ok(CrossHolderRow {
                    entity: Entity {
                        id: row.get(0)?,
                        canonical_name: row.get(1)?,
                        entity_type: row.get(2)?,
                        country: row.get(3)?,
                    },
                    ticker_count: row.get::<_, i64>(4)? as usize,
                    total_bps: row.get(5)?,
                })
            },
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
    }
    Ok(out)
}

/// Rank tickers by ownership concentration.
/// sort_by: "top1", "top3", "hhi"
pub fn query_concentration(
    conn: &Connection,
    sort_by: &str,
    limit: usize,
) -> Result<Vec<(String, ConcentrationMetrics)>, IdxError> {
    let valid = ["top1", "top3", "hhi"];
    if !valid.contains(&sort_by) {
        return Err(IdxError::ParseError(format!(
            "invalid sort_by '{sort_by}', expected one of: top1, top3, hhi"
        )));
    }

    let mut stmt = conn
        .prepare(
            "SELECT t.code, k.percentage_bps
             FROM tickers t
             JOIN ksei_holdings k ON k.ticker_id = t.id
             ORDER BY t.code ASC, k.percentage_bps DESC",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut grouped: std::collections::BTreeMap<String, Vec<i64>> =
        std::collections::BTreeMap::new();
    for row in rows {
        let (code, bps) = row.map_err(|e| IdxError::DatabaseError(e.to_string()))?;
        grouped.entry(code).or_default().push(bps);
    }

    let mut ranked: Vec<(String, ConcentrationMetrics)> = grouped
        .into_iter()
        .map(|(code, bps)| (code, compute_concentration(&bps)))
        .collect();

    ranked.sort_by(|a, b| {
        let am = &a.1;
        let bm = &b.1;
        match sort_by {
            "top1" => bm.top1_bps.cmp(&am.top1_bps),
            "top3" => bm.top3_bps.cmp(&am.top3_bps),
            "hhi" => bm.hhi.cmp(&am.hhi),
            _ => std::cmp::Ordering::Equal,
        }
        .then_with(|| a.0.cmp(&b.0))
    });

    ranked.truncate(limit);
    Ok(ranked)
}

/// List all imported releases.
pub fn query_releases(conn: &Connection) -> Result<Vec<OwnershipRelease>, IdxError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, source_url, sha256, as_of_date, row_count, imported_at
             FROM ownership_releases
             ORDER BY as_of_date DESC, imported_at DESC",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(OwnershipRelease {
                id: row.get(0)?,
                source_url: row.get(1)?,
                sha256: row.get(2)?,
                as_of_date: parse_iso_date(&row.get::<_, String>(3)?)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                row_count: row.get::<_, i64>(4)? as usize,
                imported_at: row.get(5)?,
            })
        })
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
    }
    Ok(out)
}

/// Get Bing institutional flow for a ticker.
pub fn query_bing_flow(
    conn: &Connection,
    ticker_id: i64,
) -> Result<Option<InstitutionalFlow>, IdxError> {
    let latest = conn
        .query_row(
            "SELECT MAX(report_date) FROM bing_holdings WHERE ticker_id = ?1",
            params![ticker_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let Some(period) = latest else {
        return Ok(None);
    };

    let mut stmt = conn
        .prepare(
            "SELECT b.entity_id, COALESCE(e.canonical_name, b.raw_investor_name),
                    b.investor_type, COALESCE(b.shares_held, 0), COALESCE(b.pct_ownership_bps, 0), b.signal
             FROM bing_holdings b
             LEFT JOIN entities e ON e.id = b.entity_id
             WHERE b.ticker_id = ?1 AND b.report_date = ?2
             ORDER BY COALESCE(b.pct_ownership_bps, 0) DESC, COALESCE(b.shares_held, 0) DESC",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map(params![ticker_id, &period], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut top_buyers = Vec::new();
    let mut top_sellers = Vec::new();
    let mut new_positions = Vec::new();
    let mut exited = Vec::new();

    let mut rank_counter = 1usize;
    for row in rows {
        let (entity_id, name, investor_type, shares, percentage_bps, signal_raw) =
            row.map_err(|e| IdxError::DatabaseError(e.to_string()))?;
        let Some(signal) = flow_signal_from_db(&signal_raw) else {
            continue;
        };

        let holder = HolderRow {
            rank: rank_counter,
            source: OwnershipSource::Bing,
            name,
            entity_id,
            investor_type,
            locality: None,
            shares,
            percentage_bps,
            signal: Some(signal),
        };
        rank_counter += 1;

        match signal {
            FlowSignal::Buyer => top_buyers.push(holder),
            FlowSignal::Seller => top_sellers.push(holder),
            FlowSignal::NewPosition => new_positions.push(holder),
            FlowSignal::Exited => exited.push(holder),
            FlowSignal::Holder => {}
        }
    }

    Ok(Some(InstitutionalFlow {
        period,
        top_buyers,
        top_sellers,
        new_positions,
        exited,
    }))
}

/// Compare two KSEI snapshots by date and return ownership changes.
/// Finds: new holders, exited holders, percentage increases/decreases.
pub fn query_changes(
    conn: &Connection,
    from_date: &str,
    to_date: &str,
) -> Result<Vec<ChangeRow>, IdxError> {
    let mut stmt = conn
        .prepare(
            "WITH
                from_snapshot AS (
                    SELECT
                        t.code AS ticker_code,
                        COALESCE(e.canonical_name, k.raw_investor_name) AS entity_name,
                        CASE
                            WHEN k.entity_id IS NOT NULL THEN 'id:' || k.entity_id
                            ELSE 'raw:' || UPPER(TRIM(k.raw_investor_name))
                        END AS holder_key,
                        k.percentage_bps AS old_bps
                    FROM ksei_holdings k
                    JOIN tickers t ON t.id = k.ticker_id
                    LEFT JOIN entities e ON e.id = k.entity_id
                    WHERE k.report_date = ?1
                ),
                to_snapshot AS (
                    SELECT
                        t.code AS ticker_code,
                        COALESCE(e.canonical_name, k.raw_investor_name) AS entity_name,
                        CASE
                            WHEN k.entity_id IS NOT NULL THEN 'id:' || k.entity_id
                            ELSE 'raw:' || UPPER(TRIM(k.raw_investor_name))
                        END AS holder_key,
                        k.percentage_bps AS new_bps
                    FROM ksei_holdings k
                    JOIN tickers t ON t.id = k.ticker_id
                    LEFT JOIN entities e ON e.id = k.entity_id
                    WHERE k.report_date = ?2
                ),
                keys AS (
                    SELECT ticker_code, holder_key FROM from_snapshot
                    UNION
                    SELECT ticker_code, holder_key FROM to_snapshot
                )
             SELECT
                k.ticker_code,
                COALESCE(ts.entity_name, fs.entity_name) AS entity_name,
                fs.old_bps,
                ts.new_bps
             FROM keys k
             LEFT JOIN from_snapshot fs
                    ON fs.ticker_code = k.ticker_code AND fs.holder_key = k.holder_key
             LEFT JOIN to_snapshot ts
                    ON ts.ticker_code = k.ticker_code AND ts.holder_key = k.holder_key
             WHERE
                (fs.old_bps IS NULL AND ts.new_bps IS NOT NULL)
                OR (fs.old_bps IS NOT NULL AND ts.new_bps IS NULL)
                OR (fs.old_bps IS NOT NULL AND ts.new_bps IS NOT NULL AND fs.old_bps <> ts.new_bps)
             ORDER BY k.ticker_code ASC, ABS(COALESCE(ts.new_bps, 0) - COALESCE(fs.old_bps, 0)) DESC, entity_name ASC",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map(params![from_date, to_date], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let (ticker_code, entity_name, old_bps, new_bps) =
            row.map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let change_type = match (old_bps, new_bps) {
            (None, Some(_)) => ChangeType::New,
            (Some(_), None) => ChangeType::Exited,
            (Some(old), Some(new)) if new > old => ChangeType::Increased,
            (Some(_), Some(_)) => ChangeType::Decreased,
            (None, None) => continue,
        };

        let delta_bps = new_bps.unwrap_or(0) - old_bps.unwrap_or(0);
        out.push(ChangeRow {
            ticker_code,
            entity_name,
            change_type,
            old_bps,
            new_bps,
            delta_bps,
        });
    }

    Ok(out)
}

/// List aliases that are unresolved in holdings or have low-confidence mappings.
pub fn list_unresolved(conn: &Connection, limit: usize) -> Result<Vec<UnresolvedRow>, IdxError> {
    let limit_i64 =
        i64::try_from(limit).map_err(|e| IdxError::DatabaseError(format!("invalid limit: {e}")))?;

    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT
                h.raw_name,
                h.source,
                t.code,
                e.canonical_name,
                a.confidence
             FROM (
                SELECT raw_investor_name AS raw_name, 'ksei' AS source, ticker_id, entity_id
                FROM ksei_holdings
                UNION ALL
                SELECT raw_investor_name AS raw_name, 'bing' AS source, ticker_id, entity_id
                FROM bing_holdings
             ) h
             JOIN tickers t ON t.id = h.ticker_id
             LEFT JOIN entity_aliases a ON a.raw_name = h.raw_name AND a.source = h.source
             LEFT JOIN entities e ON e.id = COALESCE(a.entity_id, h.entity_id)
             WHERE h.entity_id IS NULL OR a.entity_id IS NULL OR COALESCE(a.confidence, 0.0) < 0.8
             ORDER BY COALESCE(a.confidence, 0.0) ASC, h.raw_name ASC
             LIMIT ?1",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map(params![limit_i64], |row| {
            Ok(UnresolvedRow {
                raw_name: row.get(0)?,
                source: row.get(1)?,
                ticker_code: row.get(2)?,
                current_entity: row.get(3)?,
                confidence: row.get(4)?,
            })
        })
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
    }
    Ok(out)
}

/// Manually map a raw investor name to a canonical entity.
/// Creates the entity if it does not exist, then creates/updates aliases and unresolved holdings.
pub fn manual_map(conn: &Connection, raw_name: &str, canonical_name: &str) -> Result<(), IdxError> {
    let raw_name = raw_name.trim();
    let canonical_name = canonical_name.trim();

    if raw_name.is_empty() || canonical_name.is_empty() {
        return Err(IdxError::ParseError(
            "alias and canonical entity must be non-empty".to_string(),
        ));
    }

    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let tx_result: Result<(), IdxError> = (|| {
        let entity_id: i64 = match conn.query_row(
            "SELECT id FROM entities WHERE canonical_name = ?1",
            params![canonical_name],
            |row| row.get(0),
        ) {
            Ok(id) => id,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let now = chrono::Utc::now().timestamp();
                conn.execute(
                    "INSERT INTO entities (canonical_name, entity_type, country, created_at, updated_at)
                     VALUES (?1, NULL, NULL, ?2, ?2)",
                    params![canonical_name, now],
                )
                .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
                conn.last_insert_rowid()
            }
            Err(e) => return Err(IdxError::DatabaseError(e.to_string())),
        };

        for source in ["ksei", "bing"] {
            conn.execute(
                "INSERT INTO entity_aliases (entity_id, raw_name, source, confidence, method)
                 VALUES (?1, ?2, ?3, 1.0, 'manual')
                 ON CONFLICT(raw_name, source) DO UPDATE SET
                    entity_id = excluded.entity_id,
                    confidence = 1.0,
                    method = 'manual'",
                params![entity_id, raw_name, source],
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
        }

        conn.execute(
            "UPDATE ksei_holdings SET entity_id = ?1 WHERE raw_investor_name = ?2",
            params![entity_id, raw_name],
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        conn.execute(
            "UPDATE bing_holdings SET entity_id = ?1 WHERE raw_investor_name = ?2",
            params![entity_id, raw_name],
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let _ = search::rebuild_fts(conn);
        Ok(())
    })();

    match tx_result {
        Ok(()) => {
            conn.execute("COMMIT", [])
                .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
            Ok(())
        }
        Err(err) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(err)
        }
    }
}

/// Merge two entities by re-pointing aliases and holdings from merge_id to keep_id, then deleting merge_id.
pub fn merge_entities(conn: &Connection, keep_id: i64, merge_id: i64) -> Result<(), IdxError> {
    if keep_id == merge_id {
        return Err(IdxError::ParseError(
            "keep_id and merge_id must be different".to_string(),
        ));
    }

    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let tx_result: Result<(), IdxError> = (|| {
        let keep_exists: i64 = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM entities WHERE id = ?1)",
                params![keep_id],
                |row| row.get(0),
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
        let merge_exists: i64 = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM entities WHERE id = ?1)",
                params![merge_id],
                |row| row.get(0),
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        if keep_exists != 1 || merge_exists != 1 {
            return Err(IdxError::ParseError(format!(
                "entity not found: keep_id={keep_id}, merge_id={merge_id}"
            )));
        }

        conn.execute(
            "UPDATE entity_aliases SET entity_id = ?1 WHERE entity_id = ?2",
            params![keep_id, merge_id],
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        conn.execute(
            "UPDATE ksei_holdings SET entity_id = ?1 WHERE entity_id = ?2",
            params![keep_id, merge_id],
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        conn.execute(
            "UPDATE bing_holdings SET entity_id = ?1 WHERE entity_id = ?2",
            params![keep_id, merge_id],
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        conn.execute("DELETE FROM entities WHERE id = ?1", params![merge_id])
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let _ = search::rebuild_fts(conn);
        Ok(())
    })();

    match tx_result {
        Ok(()) => {
            conn.execute("COMMIT", [])
                .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
            Ok(())
        }
        Err(err) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(err)
        }
    }
}

/// Get ticker_id by code.
pub fn get_ticker_id(conn: &Connection, code: &str) -> Result<Option<i64>, IdxError> {
    conn.query_row(
        "SELECT id FROM tickers WHERE code = ?1",
        params![code],
        |row| row.get(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        _ => Err(IdxError::DatabaseError(e.to_string())),
    })
}

/// Compute concentration metrics from a list of percentage_bps values.
pub fn compute_concentration(percentages_bps: &[i64]) -> ConcentrationMetrics {
    let mut values = percentages_bps.to_vec();
    values.sort_by(|a, b| b.cmp(a));

    let top1_bps = values.first().copied().unwrap_or(0);
    let top3_bps: i64 = values.iter().take(3).sum();
    let total_bps: i64 = values.iter().sum();
    let hhi: i64 = values.iter().map(|p| (p * p) / 10000).sum();

    ConcentrationMetrics {
        top1_bps,
        top3_bps,
        hhi,
        free_float_bps: (10000 - total_bps).max(0),
        holder_count: values.iter().filter(|p| **p >= 100).count(),
    }
}

fn resolve_db_path() -> Result<PathBuf, IdxError> {
    if let Some(custom_path) = get_config_value("ownership.db_path")? {
        let trimmed = custom_path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    ProjectDirs::from("", "", "idx")
        .map(|dirs| dirs.data_local_dir().join("ownership.db"))
        .ok_or_else(|| IdxError::DatabaseError("unable to resolve ownership db path".to_string()))
}

fn query_ticker(conn: &Connection, code: &str) -> Result<Option<Ticker>, IdxError> {
    conn.query_row(
        "SELECT id, code, name, sector FROM tickers WHERE code = ?1",
        params![code],
        |row| {
            Ok(Ticker {
                id: row.get(0)?,
                code: row.get(1)?,
                name: row.get(2)?,
                sector: row.get(3)?,
            })
        },
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        _ => Err(IdxError::DatabaseError(e.to_string())),
    })
}

fn query_entity(conn: &Connection, entity_id: i64) -> Result<Option<Entity>, IdxError> {
    conn.query_row(
        "SELECT id, canonical_name, entity_type, country FROM entities WHERE id = ?1",
        params![entity_id],
        |row| {
            Ok(Entity {
                id: row.get(0)?,
                canonical_name: row.get(1)?,
                entity_type: row.get(2)?,
                country: row.get(3)?,
            })
        },
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        _ => Err(IdxError::DatabaseError(e.to_string())),
    })
}

fn parse_iso_date(s: &str) -> Result<NaiveDate, IdxError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| IdxError::ParseError(format!("invalid ISO date '{s}': {e}")))
}

fn locality_to_db(locality: Locality) -> &'static str {
    match locality {
        Locality::Local => "L",
        Locality::Foreign => "F",
    }
}

fn locality_from_db(value: &str) -> Option<Locality> {
    match value {
        "L" => Some(Locality::Local),
        "F" | "A" => Some(Locality::Foreign),
        _ => None,
    }
}

fn flow_signal_to_db(signal: FlowSignal) -> &'static str {
    match signal {
        FlowSignal::Holder => "holder",
        FlowSignal::Buyer => "buyer",
        FlowSignal::Seller => "seller",
        FlowSignal::NewPosition => "new_position",
        FlowSignal::Exited => "exited",
    }
}

fn flow_signal_from_db(value: &str) -> Option<FlowSignal> {
    match value {
        "holder" => Some(FlowSignal::Holder),
        "buyer" => Some(FlowSignal::Buyer),
        "seller" => Some(FlowSignal::Seller),
        "new_position" => Some(FlowSignal::NewPosition),
        "exited" => Some(FlowSignal::Exited),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rusqlite::{Connection, params};

    use crate::ownership::db::{
        compute_concentration, ensure_schema, get_ticker_id, insert_bing_holdings,
        insert_ksei_holdings, insert_release, query_concentration, query_cross_holders,
        query_ticker_holdings, release_exists, upsert_ticker,
    };
    use crate::ownership::types::{
        BingHolding, FlowSignal, KseiHolding, Locality, OwnershipRelease,
    };

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_compute_concentration_pure_function() {
        let metrics = compute_concentration(&[4000, 2500, 1000, 500]);
        assert_eq!(metrics.top1_bps, 4000);
        assert_eq!(metrics.top3_bps, 7500);
        assert_eq!(metrics.hhi, 2350);
        assert_eq!(metrics.free_float_bps, 2000);
        assert_eq!(metrics.holder_count, 4);
    }

    #[test]
    fn test_release_exists_before_and_after_insert() {
        let conn = setup();
        assert!(!release_exists(&conn, "abc").unwrap());

        let release = OwnershipRelease {
            id: 0,
            source_url: Some("https://example.com/r.pdf".to_string()),
            sha256: "abc".to_string(),
            as_of_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
            row_count: 10,
            imported_at: 1_700_000_000,
        };
        let _ = insert_release(&conn, &release).unwrap();

        assert!(release_exists(&conn, "abc").unwrap());
    }

    #[test]
    fn test_insert_fixture_ksei_and_query_ticker() {
        let conn = setup();
        let bbca_id = upsert_ticker(&conn, "BBCA", Some("BCA")).unwrap();

        conn.execute(
            "INSERT INTO entities (canonical_name, entity_type, country, created_at, updated_at)
             VALUES ('ALPHA FUND', NULL, NULL, 0, 0)",
            [],
        )
        .unwrap();
        let alpha_id = conn.last_insert_rowid();

        let holdings = vec![
            KseiHolding {
                id: 0,
                ticker_id: bbca_id,
                entity_id: Some(alpha_id),
                raw_investor_name: "PT ALPHA FUND".to_string(),
                investor_type: None,
                locality: Some(Locality::Local),
                nationality: None,
                domicile: None,
                holdings_scripless: 1_000,
                holdings_scrip: 0,
                total_shares: 1_000,
                percentage_bps: 4000,
                report_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
                release_sha256: "r1".to_string(),
            },
            KseiHolding {
                id: 0,
                ticker_id: bbca_id,
                entity_id: None,
                raw_investor_name: "BETA".to_string(),
                investor_type: None,
                locality: Some(Locality::Foreign),
                nationality: None,
                domicile: None,
                holdings_scripless: 600,
                holdings_scrip: 0,
                total_shares: 600,
                percentage_bps: 2400,
                report_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
                release_sha256: "r1".to_string(),
            },
        ];

        assert_eq!(insert_ksei_holdings(&conn, &holdings).unwrap(), 2);

        let data = query_ticker_holdings(&conn, "BBCA").unwrap();
        assert_eq!(data.ticker.code, "BBCA");
        assert_eq!(data.holders.len(), 2);
        assert_eq!(data.holders[0].percentage_bps, 4000);
        assert_eq!(data.concentration.top1_bps, 4000);
        assert_eq!(data.concentration.top3_bps, 6400);
    }

    #[test]
    fn test_duplicate_insert_ignored() {
        let conn = setup();
        let bbri_id = upsert_ticker(&conn, "BBRI", None).unwrap();

        let h = KseiHolding {
            id: 0,
            ticker_id: bbri_id,
            entity_id: None,
            raw_investor_name: "DUP".to_string(),
            investor_type: None,
            locality: None,
            nationality: None,
            domicile: None,
            holdings_scripless: 10,
            holdings_scrip: 0,
            total_shares: 10,
            percentage_bps: 100,
            report_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
            release_sha256: "same-release".to_string(),
        };

        assert_eq!(
            insert_ksei_holdings(&conn, std::slice::from_ref(&h)).unwrap(),
            1
        );
        assert_eq!(insert_ksei_holdings(&conn, &[h]).unwrap(), 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ksei_holdings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_cross_holders_same_entity_three_tickers() {
        let conn = setup();
        conn.execute(
            "INSERT INTO entities (canonical_name, entity_type, country, created_at, updated_at)
             VALUES ('OMEGA', NULL, NULL, 0, 0)",
            [],
        )
        .unwrap();
        let eid = conn.last_insert_rowid();

        for (code, bps) in [("BBCA", 1000), ("BBRI", 1100), ("BMRI", 1200)] {
            let tid = upsert_ticker(&conn, code, None).unwrap();
            let h = KseiHolding {
                id: 0,
                ticker_id: tid,
                entity_id: Some(eid),
                raw_investor_name: "OMEGA".to_string(),
                investor_type: None,
                locality: None,
                nationality: None,
                domicile: None,
                holdings_scripless: 10,
                holdings_scrip: 0,
                total_shares: 10,
                percentage_bps: bps,
                report_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
                release_sha256: format!("r-{code}"),
            };
            insert_ksei_holdings(&conn, &[h]).unwrap();
        }

        let rows = query_cross_holders(&conn, 3, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entity.id, eid);
        assert_eq!(rows[0].ticker_count, 3);
    }

    #[test]
    fn test_concentration_sorting() {
        let conn = setup();
        let aa = upsert_ticker(&conn, "AA", None).unwrap();
        let bb = upsert_ticker(&conn, "BB", None).unwrap();

        let rows = vec![
            (aa, 6000, "A1"),
            (aa, 1000, "A2"),
            (bb, 5000, "B1"),
            (bb, 3000, "B2"),
        ];

        for (tid, bps, name) in rows {
            insert_ksei_holdings(
                &conn,
                &[KseiHolding {
                    id: 0,
                    ticker_id: tid,
                    entity_id: None,
                    raw_investor_name: name.to_string(),
                    investor_type: None,
                    locality: None,
                    nationality: None,
                    domicile: None,
                    holdings_scripless: 1,
                    holdings_scrip: 0,
                    total_shares: 1,
                    percentage_bps: bps,
                    report_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
                    release_sha256: format!("{tid}-{name}"),
                }],
            )
            .unwrap();
        }

        let by_top1 = query_concentration(&conn, "top1", 10).unwrap();
        assert_eq!(by_top1[0].0, "AA");

        let by_top3 = query_concentration(&conn, "top3", 10).unwrap();
        assert_eq!(by_top3[0].0, "BB");

        let by_hhi = query_concentration(&conn, "hhi", 10).unwrap();
        assert_eq!(by_hhi[0].0, "AA");
    }

    #[test]
    fn test_insert_bing_and_query_empty_db_paths() {
        let conn = setup();

        assert!(get_ticker_id(&conn, "NONE").unwrap().is_none());
        assert!(query_cross_holders(&conn, 1, 10).unwrap().is_empty());
        assert!(query_concentration(&conn, "top1", 10).unwrap().is_empty());

        let tid = upsert_ticker(&conn, "TLKM", None).unwrap();
        let inserted = insert_bing_holdings(
            &conn,
            &[BingHolding {
                id: 0,
                ticker_id: tid,
                entity_id: None,
                raw_investor_name: "FLOW".to_string(),
                investor_type: None,
                shares_held: Some(100),
                shares_changed: Some(50),
                pct_ownership_bps: Some(123),
                value_usd: Some(10),
                report_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
                signal: FlowSignal::Buyer,
                fetched_at: 12345,
            }],
        )
        .unwrap();
        assert_eq!(inserted, 1);

        let buyers: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bing_holdings WHERE ticker_id = ?1 AND signal = 'buyer'",
                params![tid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(buyers, 1);
    }
}
