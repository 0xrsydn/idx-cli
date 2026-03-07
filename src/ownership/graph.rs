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
        return Err(IdxError::ParseError(
            "graph root cannot be empty".to_string(),
        ));
    }

    let root_node_id = detect_root_node(conn, root)?;

    let mut visited: BTreeSet<String> = BTreeSet::new();
    {
        let mut stmt = conn
            .prepare(
                "WITH RECURSIVE
                    all_edges AS (
                        SELECT
                            'entity:' || k.entity_id AS from_id,
                            'ticker:' || t.code AS to_id
                        FROM ksei_holdings k
                        JOIN tickers t ON t.id = k.ticker_id
                        WHERE k.entity_id IS NOT NULL

                        UNION

                        SELECT
                            'entity:' || b.entity_id AS from_id,
                            'ticker:' || t.code AS to_id
                        FROM bing_holdings b
                        JOIN tickers t ON t.id = b.ticker_id
                        WHERE b.entity_id IS NOT NULL
                    ),
                    neighbors AS (
                        SELECT from_id AS a, to_id AS b FROM all_edges
                        UNION
                        SELECT to_id AS a, from_id AS b FROM all_edges
                    ),
                    walk(node_id, depth, path) AS (
                        SELECT ?1 AS node_id, 0 AS depth, ?1 AS path
                        UNION ALL
                        SELECT n.b, w.depth + 1, w.path || '>' || n.b
                        FROM walk w
                        JOIN neighbors n ON n.a = w.node_id
                        WHERE w.depth < ?2
                          AND instr(w.path, n.b) = 0
                    )
                    SELECT DISTINCT node_id
                    FROM walk",
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map(params![root_node_id, depth as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        for row in rows {
            visited.insert(row.map_err(|e| IdxError::DatabaseError(e.to_string()))?);
        }
    }

    let mut edges = query_all_edges(conn)?;
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
        rels.sort_by(|a, b| b.percentage_bps.cmp(&a.percentage_bps));
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

    let q = format!("%{}%", root);
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

    {
        let mut stmt = conn
            .prepare(
                "SELECT 'entity:' || k.entity_id,
                        'ticker:' || t.code,
                        k.percentage_bps
                 FROM ksei_holdings k
                 JOIN tickers t ON t.id = k.ticker_id
                 WHERE k.entity_id IS NOT NULL",
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
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
                "SELECT 'entity:' || b.entity_id,
                        'ticker:' || t.code,
                        COALESCE(b.pct_ownership_bps, 0)
                 FROM bing_holdings b
                 JOIN tickers t ON t.id = b.ticker_id
                 WHERE b.entity_id IS NOT NULL",
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
