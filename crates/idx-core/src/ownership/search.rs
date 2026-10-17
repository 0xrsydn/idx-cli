use rusqlite::{Connection, params};

use crate::error::IdxError;
use crate::ownership::types::Entity;

/// Investors whose canonical name or any recorded alias (the raw names KSEI
/// prints, such as `PT ASTRA INTERNATIONAL TBK`) contains `query`, ignoring
/// ASCII case. Ranked by an exact name, then an exact alias, then a name
/// prefix, then shorter names.
pub fn search_entities(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<Entity>, IdxError> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let limit =
        i64::try_from(limit).map_err(|e| IdxError::DatabaseError(format!("invalid limit: {e}")))?;
    // `%` and `_` in the query are literal characters, not LIKE wildcards.
    let escaped = q
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let contains = format!("%{escaped}%");
    let prefix = format!("{escaped}%");

    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.canonical_name, e.entity_type, e.country,
                    MIN(CASE
                        WHEN UPPER(e.canonical_name) = UPPER(?1) THEN 0
                        WHEN UPPER(a.raw_name) = UPPER(?1) THEN 1
                        WHEN e.canonical_name LIKE ?3 ESCAPE '\\' THEN 2
                        ELSE 3
                    END) AS match_rank
             FROM entities e
             LEFT JOIN entity_aliases a ON a.entity_id = e.id
             WHERE e.canonical_name LIKE ?2 ESCAPE '\\'
                OR a.raw_name LIKE ?2 ESCAPE '\\'
             GROUP BY e.id
             ORDER BY match_rank, LENGTH(e.canonical_name), e.canonical_name
             LIMIT ?4",
        )
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    let rows = stmt
        .query_map(params![q, contains, prefix, limit], |row| {
            Ok(Entity {
                id: row.get(0)?,
                canonical_name: row.get(1)?,
                entity_type: row.get(2)?,
                country: row.get(3)?,
            })
        })
        .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

    rows.map(|row| row.map_err(|e| IdxError::DatabaseError(e.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};

    use super::search_entities;
    use crate::ownership::db::ensure_schema;

    fn conn_with(entities: &[(&str, &[&str])]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        for (name, aliases) in entities {
            conn.execute(
                "INSERT INTO entities (canonical_name, created_at, updated_at) VALUES (?1, 0, 0)",
                params![name],
            )
            .unwrap();
            let id = conn.last_insert_rowid();
            for alias in *aliases {
                conn.execute(
                    "INSERT INTO entity_aliases (entity_id, raw_name, source, method)
                     VALUES (?1, ?2, 'ksei', 'exact')",
                    params![id, alias],
                )
                .unwrap();
            }
        }
        conn
    }

    fn names(conn: &Connection, query: &str) -> Vec<String> {
        search_entities(conn, query, 10)
            .unwrap()
            .into_iter()
            .map(|e| e.canonical_name)
            .collect()
    }

    #[test]
    fn finds_investors_by_alias() {
        let conn = conn_with(&[
            ("ASTRA INTERNATIONAL", &["PT ASTRA INTERNATIONAL TBK"]),
            ("ASTRA OTOPARTS", &[]),
        ]);

        assert_eq!(
            names(&conn, "pt astra international tbk"),
            ["ASTRA INTERNATIONAL"]
        );
    }

    #[test]
    fn ranks_exact_then_prefix_then_shorter_and_lists_each_once() {
        let conn = conn_with(&[
            ("SALIM GROUP HOLDINGS", &["PT SALIM", "SALIM"]),
            ("ANTHONI SALIM", &[]),
            ("SALIM", &[]),
        ]);

        assert_eq!(
            names(&conn, "salim"),
            ["SALIM", "SALIM GROUP HOLDINGS", "ANTHONI SALIM"]
        );
    }

    #[test]
    fn treats_like_wildcards_literally_and_empty_queries_as_no_match() {
        let conn = conn_with(&[("ALPHA_FUND", &[]), ("ALPHAXFUND", &[]), ("BETA 100%", &[])]);

        assert_eq!(names(&conn, "alpha_fund"), ["ALPHA_FUND"]);
        assert_eq!(names(&conn, "100%"), ["BETA 100%"]);
        assert!(names(&conn, "  ").is_empty());
    }
}
