use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, params};

use crate::error::IdxError;
use crate::ownership::types::{GraphEdge, GraphNode, GraphNodeType, OwnershipSource};

/// Query ownership graph starting from a ticker or entity, traversing N hops.
/// Root can be ticker code (`BBCA`) or entity name — auto-detect.
pub fn query_ownership_graph(
    conn: &Connection,
    root: &str,
    depth: usize,
) -> Result<(Vec<GraphNode>, Vec<GraphEdge>), IdxError> {
    let root = root.trim();
    if root.is_empty() {
        return Err(IdxError::InvalidInput(
            "graph root cannot be empty".to_string(),
        ));
    }

    let root_node_id = detect_root_node(conn, root)?;

    // Breadth-first over the latest edges with an exact visited set: linear
    // in the graph size, unlike enumerating every path, and a node is never
    // mistaken for one whose id merely contains it (`entity:1` in `entity:10`).
    let mut edges = query_all_edges(conn)?;
    let mut neighbors: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &edges {
        neighbors.entry(&edge.from).or_default().push(&edge.to);
        neighbors.entry(&edge.to).or_default().push(&edge.from);
    }
    let mut visited: BTreeSet<String> = BTreeSet::from([root_node_id.clone()]);
    let mut frontier = vec![root_node_id.as_str()];
    for _ in 0..depth {
        let mut next = Vec::new();
        for node in frontier {
            for &neighbor in neighbors.get(node).into_iter().flatten() {
                if visited.insert(neighbor.to_string()) {
                    next.push(neighbor);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    edges.retain(|edge| visited.contains(&edge.from) && visited.contains(&edge.to));

    let nodes = build_nodes(conn, &visited)?;

    Ok((nodes, edges))
}

/// Format graph as ASCII tree for terminal display.
pub fn format_graph_text(nodes: &[GraphNode], edges: &[GraphEdge]) -> String {
    if nodes.is_empty() {
        return "(empty graph)".to_string();
    }

    let labels: HashMap<&str, (&str, GraphNodeType)> = nodes
        .iter()
        .map(|n| (n.id.as_str(), (n.label.as_str(), n.node_type)))
        .collect();

    let mut ticker_to_entities: BTreeMap<&str, Vec<&GraphEdge>> = BTreeMap::new();
    for edge in edges {
        if let Some((_, GraphNodeType::Ticker)) = labels.get(edge.to.as_str()) {
            ticker_to_entities
                .entry(edge.to.as_str())
                .or_default()
                .push(edge);
        }
    }

    let mut out = String::new();
    out.push_str(&format!("nodes: {}  edges: {}\n", nodes.len(), edges.len()));

    for (ticker_id, mut rels) in ticker_to_entities {
        rels.sort_by_key(|edge| std::cmp::Reverse(edge.percentage_bps));
        let ticker_label = labels.get(ticker_id).map(|v| v.0).unwrap_or(ticker_id);
        out.push_str(&format!("\n{ticker_label} [TICKER]\n"));

        for (idx, edge) in rels.iter().enumerate() {
            let holder_label = labels.get(edge.from.as_str()).map(|v| v.0).unwrap_or("?");
            let branch = if idx + 1 == rels.len() {
                "└─"
            } else {
                "├─"
            };
            out.push_str(&format!(
                "{branch} {holder_label} ({:.2}%, {})\n",
                edge.percentage_bps as f64 / 100.0,
                source_label(edge.source)
            ));
        }
    }

    if edges.is_empty() {
        out.push_str("\n(no ownership edges found)\n");
    }

    out
}

/// Format graph as Graphviz DOT for export.
pub fn format_graph_dot(nodes: &[GraphNode], edges: &[GraphEdge]) -> String {
    let mut out = String::new();
    out.push_str("digraph ownership {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str("  graph [fontname=\"Helvetica\"];\n");
    out.push_str("  node [fontname=\"Helvetica\"];\n");
    out.push_str("  edge [fontname=\"Helvetica\"];\n\n");

    for node in nodes {
        let shape = match node.node_type {
            GraphNodeType::Entity => "ellipse",
            GraphNodeType::Ticker => "box",
        };
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\", shape={}];\n",
            escape_dot(&node.id),
            escape_dot(&node.label),
            shape
        ));
    }

    out.push('\n');
    for edge in edges {
        out.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{:.2}% ({})\"];\n",
            escape_dot(&edge.from),
            escape_dot(&edge.to),
            edge.percentage_bps as f64 / 100.0,
            source_label(edge.source)
        ));
    }

    out.push_str("}\n");
    out
}

fn detect_root_node(conn: &Connection, root: &str) -> Result<String, IdxError> {
    let maybe_ticker = conn
        .query_row(
            "SELECT code FROM tickers WHERE UPPER(code) = UPPER(?1) LIMIT 1",
            params![root],
            |row| row.get::<_, String>(0),
        )
        .ok();

    if let Some(code) = maybe_ticker {
        return Ok(format!("ticker:{code}"));
    }

    let q = format!("%{root}%");
    let maybe_entity_id = conn
        .query_row(
            "SELECT id
             FROM entities
             WHERE canonical_name LIKE ?1 COLLATE NOCASE
             ORDER BY LENGTH(canonical_name) ASC, canonical_name ASC
             LIMIT 1",
            params![q],
            |row| row.get::<_, i64>(0),
        )
        .ok();

    if let Some(entity_id) = maybe_entity_id {
        return Ok(format!("entity:{entity_id}"));
    }

    Err(IdxError::ParseError(format!(
        "graph root not found as ticker or entity: {root}"
    )))
}

fn query_all_edges(conn: &Connection) -> Result<Vec<GraphEdge>, IdxError> {
    let mut out = Vec::new();

    if let Ok(release_sha) = conn.query_row(
        "SELECT sha256
             FROM ownership_releases
             ORDER BY as_of_date DESC, imported_at DESC, id DESC
             LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    ) {
        let mut stmt = conn
            .prepare(
                "SELECT 'entity:' || k.entity_id,
                        'ticker:' || t.code,
                        k.percentage_bps
                 FROM ksei_holdings k
                 JOIN tickers t ON t.id = k.ticker_id
                 WHERE k.entity_id IS NOT NULL
                   AND k.release_sha256 = ?1",
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map(params![release_sha], |row| {
                Ok(GraphEdge {
                    from: row.get(0)?,
                    to: row.get(1)?,
                    percentage_bps: row.get(2)?,
                    source: OwnershipSource::Ksei,
                })
            })
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        for row in rows {
            out.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "WITH latest_bing AS (
                    SELECT ticker_id, MAX(report_date) AS report_date
                    FROM bing_holdings
                    GROUP BY ticker_id
                 )
                 SELECT 'entity:' || b.entity_id,
                        'ticker:' || t.code,
                        COALESCE(b.pct_ownership_bps, 0)
                 FROM bing_holdings b
                 JOIN latest_bing lb
                   ON lb.ticker_id = b.ticker_id
                  AND lb.report_date = b.report_date
                 JOIN tickers t ON t.id = b.ticker_id
                 WHERE b.entity_id IS NOT NULL
                   AND b.signal = 'holder'",
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(GraphEdge {
                    from: row.get(0)?,
                    to: row.get(1)?,
                    percentage_bps: row.get(2)?,
                    source: OwnershipSource::Bing,
                })
            })
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        for row in rows {
            out.push(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
        }
    }

    dedup_edges(out)
}

fn dedup_edges(edges: Vec<GraphEdge>) -> Result<Vec<GraphEdge>, IdxError> {
    let mut seen: HashSet<(String, String, i64, &'static str)> = HashSet::new();
    let mut out = Vec::new();

    for edge in edges {
        let key = (
            edge.from.clone(),
            edge.to.clone(),
            edge.percentage_bps,
            source_label(edge.source),
        );
        if seen.insert(key) {
            out.push(edge);
        }
    }

    Ok(out)
}

fn build_nodes(conn: &Connection, node_ids: &BTreeSet<String>) -> Result<Vec<GraphNode>, IdxError> {
    let mut nodes = Vec::new();

    for node_id in node_ids {
        if let Some(entity_id) = node_id.strip_prefix("entity:") {
            let entity_id_num = entity_id.parse::<i64>().map_err(|e| {
                IdxError::ParseError(format!("invalid entity node id '{node_id}': {e}"))
            })?;
            let label = conn
                .query_row(
                    "SELECT canonical_name FROM entities WHERE id = ?1",
                    params![entity_id_num],
                    |row| row.get::<_, String>(0),
                )
                .unwrap_or_else(|_| format!("entity:{entity_id_num}"));
            nodes.push(GraphNode {
                id: node_id.clone(),
                label,
                node_type: GraphNodeType::Entity,
            });
            continue;
        }

        if let Some(code) = node_id.strip_prefix("ticker:") {
            nodes.push(GraphNode {
                id: node_id.clone(),
                label: code.to_string(),
                node_type: GraphNodeType::Ticker,
            });
        }
    }

    Ok(nodes)
}

fn source_label(source: OwnershipSource) -> &'static str {
    match source {
        OwnershipSource::Ksei => "ksei",
        OwnershipSource::Bing => "bing",
    }
}

fn escape_dot(value: &str) -> String {
    value.replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rusqlite::Connection;

    use crate::ownership::db::{ensure_schema, upsert_ticker, write_ksei_release};
    use crate::ownership::types::{KseiHolding, OwnershipRelease};

    use super::query_ownership_graph;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn graph_reaches_holders_whose_id_is_a_prefix_of_a_visited_one() {
        // From entity:10 via ticker:BB to entity:1; "entity:1" is a substring
        // of "entity:10", which the old path check treated as visited.
        let conn = setup();
        let bb = upsert_ticker(&conn, "BB", None).unwrap();
        conn.execute(
            "INSERT INTO entities (id, canonical_name, created_at, updated_at)
             VALUES (10, 'HOLDER TEN', 0, 0), (1, 'HOLDER ONE', 0, 0)",
            [],
        )
        .unwrap();
        let release = OwnershipRelease {
            id: 0,
            source_url: None,
            sha256: "prefix-graph".to_string(),
            as_of_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
            row_count: 2,
            imported_at: 1,
        };
        let row = |entity_id: i64, name: &str| KseiHolding {
            id: 0,
            ticker_id: bb,
            entity_id: Some(entity_id),
            raw_investor_name: name.to_string(),
            investor_type: None,
            locality: None,
            nationality: None,
            domicile: None,
            holdings_scripless: 1,
            holdings_scrip: 0,
            total_shares: 1,
            percentage_bps: 600,
            report_date: release.as_of_date,
            release_sha256: release.sha256.clone(),
        };
        write_ksei_release(
            &conn,
            &release,
            &[row(10, "HOLDER TEN"), row(1, "HOLDER ONE")],
            false,
        )
        .unwrap();

        let (nodes, edges) = query_ownership_graph(&conn, "HOLDER TEN", 2).unwrap();

        let mut labels: Vec<_> = nodes.iter().map(|node| node.label.as_str()).collect();
        labels.sort();
        assert_eq!(labels, ["BB", "HOLDER ONE", "HOLDER TEN"]);
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn graph_uses_latest_ksei_release_only() {
        let conn = setup();
        let aa = upsert_ticker(&conn, "AA", None).unwrap();

        conn.execute(
            "INSERT INTO entities (canonical_name, entity_type, country, created_at, updated_at)
             VALUES ('OLDER HOLDER', NULL, NULL, 0, 0)",
            [],
        )
        .unwrap();
        let older_entity = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO entities (canonical_name, entity_type, country, created_at, updated_at)
             VALUES ('LATEST HOLDER', NULL, NULL, 0, 0)",
            [],
        )
        .unwrap();
        let latest_entity = conn.last_insert_rowid();

        let old_release = OwnershipRelease {
            id: 0,
            source_url: None,
            sha256: "old-graph".to_string(),
            as_of_date: NaiveDate::from_ymd_opt(2026, 1, 30).unwrap(),
            row_count: 1,
            imported_at: 1,
        };
        let old_rows = vec![KseiHolding {
            id: 0,
            ticker_id: aa,
            entity_id: Some(older_entity),
            raw_investor_name: "OLDER HOLDER".to_string(),
            investor_type: None,
            locality: None,
            nationality: None,
            domicile: None,
            holdings_scripless: 1,
            holdings_scrip: 0,
            total_shares: 1,
            percentage_bps: 1000,
            report_date: old_release.as_of_date,
            release_sha256: old_release.sha256.clone(),
        }];
        write_ksei_release(&conn, &old_release, &old_rows, false).unwrap();

        let latest_release = OwnershipRelease {
            id: 0,
            source_url: None,
            sha256: "latest-graph".to_string(),
            as_of_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
            row_count: 1,
            imported_at: 2,
        };
        let latest_rows = vec![KseiHolding {
            id: 0,
            ticker_id: aa,
            entity_id: Some(latest_entity),
            raw_investor_name: "LATEST HOLDER".to_string(),
            investor_type: None,
            locality: None,
            nationality: None,
            domicile: None,
            holdings_scripless: 1,
            holdings_scrip: 0,
            total_shares: 1,
            percentage_bps: 1200,
            report_date: latest_release.as_of_date,
            release_sha256: latest_release.sha256.clone(),
        }];
        write_ksei_release(&conn, &latest_release, &latest_rows, false).unwrap();

        let (nodes, edges) = query_ownership_graph(&conn, "AA", 1).unwrap();
        assert!(nodes.iter().any(|node| node.label == "LATEST HOLDER"));
        assert!(!nodes.iter().any(|node| node.label == "OLDER HOLDER"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].percentage_bps, 1200);
    }
}
