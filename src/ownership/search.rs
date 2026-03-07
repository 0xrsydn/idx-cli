use rusqlite::{Connection, params};

use crate::error::IdxError;
use crate::ownership::types::Entity;

pub fn fts_search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<Entity>, IdxError> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }

    let limit_i64 =
        i64::try_from(limit).map_err(|e| IdxError::DatabaseError(format!("invalid limit: {e}")))?;

    // Ensure entity_fts has content. For external-content FTS table, populate manually.
    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM entity_fts", [], |row| row.get(0))
        .unwrap_or(0);

    if fts_count == 0 {
        let _ = rebuild_fts(conn);
    }

    if let Ok(rows) = fts_query(conn, q, limit_i64)
        && !rows.is_empty()
    {
        return Ok(rows);
    }

    like_query(conn, q, limit_i64)
}

fn fts_query(conn: &Connection, query: &str, limit: i64) -> Result<Vec<Entity>, IdxError> {
    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.canonical_name, e.entity_type, e.country
             FROM entity_fts f
             JOIN entities e ON e.id = f.rowid
             WHERE entity_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map(params![query, limit], |row| {
            Ok(Entity {
                id: row.get(0)?,
                canonical_name: row.get(1)?,
                entity_type: row.get(2)?,
                country: row.get(3)?,
            })
        })
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
    }
    Ok(out)
}

fn like_query(conn: &Connection, query: &str, limit: i64) -> Result<Vec<Entity>, IdxError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, canonical_name, entity_type, country
             FROM entities
             WHERE canonical_name LIKE ?1 COLLATE NOCASE
             ORDER BY canonical_name ASC
             LIMIT ?2",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let like = format!("%{query}%");
    let rows = stmt
        .query_map(params![like, limit], |row| {
            Ok(Entity {
                id: row.get(0)?,
                canonical_name: row.get(1)?,
                entity_type: row.get(2)?,
                country: row.get(3)?,
            })
        })
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
    }
    Ok(out)
}

pub fn rebuild_fts(conn: &Connection) -> Result<(), IdxError> {
    conn.execute("DELETE FROM entity_fts", [])
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    conn.execute(
        "INSERT INTO entity_fts(rowid, canonical_name, aliases)
         SELECT e.id, e.canonical_name, COALESCE(a.aliases, '')
         FROM entities e
         LEFT JOIN (
             SELECT entity_id, GROUP_CONCAT(raw_name, ' ') AS aliases
             FROM entity_aliases
             GROUP BY entity_id
         ) a ON a.entity_id = e.id",
        [],
    )
    .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    Ok(())
}
