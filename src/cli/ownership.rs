use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use clap::{Args, Subcommand};
use comfy_table::{Cell, ContentArrangement, Table, presets::UTF8_FULL};
use directories::ProjectDirs;
use owo_colors::OwoColorize;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::IdxConfig;
use crate::error::IdxError;
use crate::output::OutputFormat;
use crate::output::json;
use crate::output::table::format_idr;
use crate::ownership::types::{
    ChangeType, FlowSignal, HolderRow, KseiHolding, OwnershipRelease, OwnershipSource,
};
use crate::ownership::{db, entities, graph, parser, remote, search, snapshot};

#[derive(Debug, Args)]
pub struct OwnershipCmd {
    #[command(subcommand)]
    pub command: OwnershipCommand,
}

#[derive(Debug, Subcommand)]
pub enum OwnershipCommand {
    /// Discover the latest IDX-hosted ownership report URLs.
    Discover(DiscoverArgs),
    /// Import ownership data from KSEI PDF or Bing API.
    Import(ImportArgs),
    /// Install or refresh a maintained ownership SQLite snapshot.
    Sync(SyncArgs),
    /// Show all holders for a ticker (KSEI + Bing combined).
    Ticker(TickerArgs),
    /// Show all holdings for an entity across tickers.
    Entity(EntityArgs),
    /// Search entities by name.
    Search(SearchArgs),
    /// Rank entities by cross-ownership breadth.
    CrossHolders(CrossHolderArgs),
    /// Rank tickers by ownership concentration.
    Concentration(ConcentrationArgs),
    /// Show Bing institutional flow for a ticker.
    Flow(FlowArgs),
    /// Diff two KSEI releases.
    Changes(ChangesArgs),
    /// Ownership network graph.
    Graph(GraphArgs),
    /// Manual entity resolution workflow.
    Resolve(ResolveArgs),
    /// List imported KSEI releases.
    Releases,
}

#[derive(Debug, Args)]
pub struct DiscoverArgs {
    /// Report family to discover: above1 (default), all, above5, or investor-type.
    #[arg(long, default_value = "above1")]
    pub family: String,
    /// Maximum number of discovered report URLs to print.
    #[arg(long, default_value_t = 6)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// URL to a remote ownership PDF.
    #[arg(long)]
    pub url: Option<String>,
    /// Path to local KSEI PDF file.
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// Fetch Bing institutional data for these symbols.
    #[arg(long, value_delimiter = ',')]
    pub fetch_bing: Option<Vec<String>>,
    /// Re-import even if already imported.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Snapshot manifest location (URL or local path).
    #[arg(long)]
    pub manifest: Option<String>,
    /// Replace the local DB even when already current or newer.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct TickerArgs {
    pub symbol: String,
    #[arg(long, default_value = "all")]
    pub source: String,
}

#[derive(Debug, Args)]
pub struct EntityArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct CrossHolderArgs {
    #[arg(long, default_value_t = 20)]
    pub top: usize,
    #[arg(long, default_value_t = 2)]
    pub min_tickers: usize,
}

#[derive(Debug, Args)]
pub struct ConcentrationArgs {
    #[arg(long, default_value = "hhi")]
    pub by: String,
    #[arg(long, default_value_t = 20)]
    pub top: usize,
}

#[derive(Debug, Args)]
pub struct FlowArgs {
    pub symbol: String,
}

#[derive(Debug, Args)]
pub struct ChangesArgs {
    #[arg(long)]
    pub from: String,
    #[arg(long)]
    pub to: String,
}

#[derive(Debug, Args)]
pub struct GraphArgs {
    /// Ticker code or entity name to start from.
    pub root: String,
    /// Traversal depth (default 2).
    #[arg(long, default_value_t = 2)]
    pub depth: usize,
    /// Output format: table or dot (Graphviz).
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Debug, Args)]
pub struct ResolveArgs {
    #[command(subcommand)]
    pub command: ResolveCommand,
}

#[derive(Debug, Subcommand)]
pub enum ResolveCommand {
    /// List unresolved or low-confidence entity aliases.
    Unresolved {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Manually map a raw investor name to a canonical entity name.
    Map {
        /// The raw name as it appears in the data.
        alias: String,
        /// The canonical entity name to map to.
        entity: String,
    },
    /// Merge two entities (keep first, merge second into it).
    Merge {
        /// Entity ID to keep.
        keep: i64,
        /// Entity ID to merge and delete.
        merge: i64,
    },
}

pub fn handle(cmd: &OwnershipCommand, config: &IdxConfig) -> Result<(), IdxError> {
    match cmd {
        OwnershipCommand::Discover(args) => handle_discover(args, config),
        OwnershipCommand::Import(args) => handle_import(args, config),
        OwnershipCommand::Sync(args) => handle_sync(args, config),
        OwnershipCommand::Ticker(args) => handle_ticker(args, config),
        OwnershipCommand::Entity(args) => handle_entity(args, config),
        OwnershipCommand::Search(args) => handle_search(args, config),
        OwnershipCommand::CrossHolders(args) => handle_cross_holders(args, config),
        OwnershipCommand::Concentration(args) => handle_concentration(args, config),
        OwnershipCommand::Flow(args) => handle_flow(args, config),
        OwnershipCommand::Changes(args) => handle_changes(args, config),
        OwnershipCommand::Graph(args) => handle_graph(args, config),
        OwnershipCommand::Resolve(args) => handle_resolve(args, config),
        OwnershipCommand::Releases => handle_releases(config),
    }
}

fn handle_sync(args: &SyncArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let manifest_source = snapshot::resolve_manifest_source(args.manifest.as_deref())?;
    let db_path = db::db_path(config)?;
    let result = snapshot::sync_snapshot(&manifest_source, &db_path, args.force)?;

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&result);
    }

    match result.action {
        snapshot::OwnershipSyncAction::Installed => {
            println!(
                "Installed ownership snapshot {} ({} release(s), {} tickers) into {}.",
                result.latest_as_of_date.format("%Y-%m-%d"),
                result.release_count,
                result.ticker_count,
                result.db_path
            );
        }
        snapshot::OwnershipSyncAction::Updated => {
            println!(
                "Updated ownership snapshot to {} ({} release(s), {} tickers) in {}.",
                result.latest_as_of_date.format("%Y-%m-%d"),
                result.release_count,
                result.ticker_count,
                result.db_path
            );
        }
        snapshot::OwnershipSyncAction::Refreshed => {
            println!(
                "Refreshed ownership snapshot {} in {}.",
                result.latest_as_of_date.format("%Y-%m-%d"),
                result.db_path
            );
        }
        snapshot::OwnershipSyncAction::NoChange
        | snapshot::OwnershipSyncAction::SkippedNewer
        | snapshot::OwnershipSyncAction::SkippedDiverged => {
            println!("{}", result.reason);
        }
    }

    Ok(())
}

fn handle_discover(args: &DiscoverArgs, config: &IdxConfig) -> Result<(), IdxError> {
    if args.limit == 0 {
        return Err(IdxError::ParseError(
            "--limit must be greater than 0".to_string(),
        ));
    }

    let family = parse_discovery_family(&args.family)?;
    let reports = remote::discover_idx_ownership_reports(family, args.limit)?;

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&reports);
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "DATE", "FAMILY", "STATUS", "KIND", "FILE", "TITLE", "URL",
        ]);

    for report in reports {
        table.add_row(vec![
            Cell::new(report.publish_date.split('T').next().unwrap_or("-")),
            Cell::new(report.family.label()),
            Cell::new(report.status.label()),
            Cell::new(if report.is_attachment {
                "attachment"
            } else {
                "main"
            }),
            Cell::new(report.original_filename.unwrap_or_else(|| "-".to_string())),
            Cell::new(report.title),
            Cell::new(report.pdf_url),
        ]);
    }

    println!("{table}");
    Ok(())
}

fn handle_ticker(args: &TickerArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let symbol = args.symbol.trim().to_uppercase();
    let source = args.source.trim().to_lowercase();
    if !["all", "ksei", "bing"].contains(&source.as_str()) {
        return Err(IdxError::ParseError(
            "invalid --source, expected: all|ksei|bing".to_string(),
        ));
    }

    let mut data = db::query_ticker_holdings(&conn, &symbol)?;
    if data.holders.is_empty() {
        return Err(IdxError::ParseError(format!(
            "no ownership data found for symbol {symbol}"
        )));
    }

    if source != "all" {
        data.holders.retain(|h| match source.as_str() {
            "ksei" => matches!(h.source, OwnershipSource::Ksei),
            "bing" => matches!(h.source, OwnershipSource::Bing),
            _ => true,
        });
        for (i, row) in data.holders.iter_mut().enumerate() {
            row.rank = i + 1;
        }
        let percentages: Vec<i64> = data.holders.iter().map(|h| h.percentage_bps).collect();
        data.concentration = db::compute_concentration(&percentages);
    }

    if data.holders.is_empty() {
        return Err(IdxError::ParseError(format!(
            "no {source} ownership data found for symbol {symbol}"
        )));
    }

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&data);
    }

    println!(
        "{} {}  KSEI={}  Bing={}",
        "Ownership:".bold(),
        data.ticker.code.bold(),
        data.ksei_as_of
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "-".to_string()),
        data.bing_as_of.unwrap_or_else(|| "-".to_string())
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "RANK", "SOURCE", "INVESTOR", "TYPE", "L/F", "SHARES", "%", "SIGNAL",
        ]);

    for h in &data.holders {
        table.add_row(vec![
            Cell::new(h.rank),
            Cell::new(match h.source {
                OwnershipSource::Ksei => "KSEI",
                OwnershipSource::Bing => "BING",
            }),
            Cell::new(&h.name),
            Cell::new(h.investor_type.clone().unwrap_or_else(|| "-".to_string())),
            Cell::new(match h.locality {
                Some(crate::ownership::types::Locality::Local) => "L",
                Some(crate::ownership::types::Locality::Foreign) => "F",
                None => "-",
            }),
            Cell::new(format_idr(h.shares)),
            Cell::new(format_bps(h.percentage_bps)),
            Cell::new(format_signal(h.signal)),
        ]);
    }
    println!("{table}");

    println!(
        "{} top1={}  top3={}  hhi={}  free_float={}  holders={}",
        "Concentration:".bold(),
        format_bps(data.concentration.top1_bps),
        format_bps(data.concentration.top3_bps),
        data.concentration.hhi,
        format_bps(data.concentration.free_float_bps),
        data.concentration.holder_count
    );

    Ok(())
}

fn handle_entity(args: &EntityArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let q = format!("%{}%", args.name.trim());

    let entity = conn
        .query_row(
            "SELECT id, canonical_name FROM entities WHERE canonical_name LIKE ?1 COLLATE NOCASE ORDER BY LENGTH(canonical_name) ASC LIMIT 1",
            rusqlite::params![q],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|_| IdxError::ParseError(format!("entity not found: {}", args.name)))?;

    let data = db::query_entity_holdings(&conn, entity.0)?;

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&data);
    }

    println!("{} {}", "Entity:".bold(), entity.1.bold());
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["TICKER", "SOURCE", "SHARES", "%", "REPORT_DATE"]);

    for row in &data.holdings {
        table.add_row(vec![
            Cell::new(&row.ticker.code),
            Cell::new(match row.source {
                OwnershipSource::Ksei => "KSEI",
                OwnershipSource::Bing => "BING",
            }),
            Cell::new(format_idr(row.shares)),
            Cell::new(format_bps(row.percentage_bps)),
            Cell::new(row.report_date.format("%Y-%m-%d").to_string()),
        ]);
    }
    println!("{table}");
    println!("{} {}", "Total tickers:".bold(), data.ticker_count);

    Ok(())
}

fn handle_search(args: &SearchArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let entities = search::fts_search(&conn, &args.query, args.limit)?;

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&entities);
    }

    if entities.is_empty() {
        println!("No entities found for query: {}", args.query);
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["ENTITY", "TYPE", "COUNTRY"]);

    for entity in &entities {
        table.add_row(vec![
            Cell::new(&entity.canonical_name),
            Cell::new(
                entity
                    .entity_type
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ),
            Cell::new(entity.country.clone().unwrap_or_else(|| "-".to_string())),
        ]);
    }
    println!("{table}");
    Ok(())
}

fn handle_cross_holders(args: &CrossHolderArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let rows = db::query_cross_holders(&conn, args.min_tickers, args.top)?;

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&rows);
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["RANK", "ENTITY", "TICKERS", "TOTAL_BPS"]);

    for (i, row) in rows.iter().enumerate() {
        table.add_row(vec![
            Cell::new(i + 1),
            Cell::new(&row.entity.canonical_name),
            Cell::new(row.ticker_count),
            Cell::new(format_bps(row.total_bps)),
        ]);
    }

    println!("{table}");
    Ok(())
}

fn handle_concentration(args: &ConcentrationArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let by = args.by.trim().to_lowercase();
    if !["top1", "top3", "hhi"].contains(&by.as_str()) {
        return Err(IdxError::ParseError(
            "invalid --by, expected: top1|top3|hhi".to_string(),
        ));
    }

    let rows = db::query_concentration(&conn, &by, args.top)?;

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&rows);
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "RANK",
            "TICKER",
            "TOP1%",
            "TOP3%",
            "HHI",
            "FREE_FLOAT%",
            "HOLDERS",
        ]);

    for (idx, (ticker, m)) in rows.iter().enumerate() {
        table.add_row(vec![
            Cell::new(idx + 1),
            Cell::new(ticker),
            Cell::new(format_bps(m.top1_bps)),
            Cell::new(format_bps(m.top3_bps)),
            Cell::new(m.hhi),
            Cell::new(format_bps(m.free_float_bps)),
            Cell::new(m.holder_count),
        ]);
    }

    println!("{table}");
    Ok(())
}

fn handle_flow(args: &FlowArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let symbol = args.symbol.trim().to_uppercase();
    let ticker_id = db::get_ticker_id(&conn, &symbol)?
        .ok_or_else(|| IdxError::SymbolNotFound(symbol.clone()))?;

    let flow = db::query_bing_flow(&conn, ticker_id)?;
    let Some(flow) = flow else {
        println!("No institutional flow data. Run: idx ownership import --fetch-bing {symbol}");
        return Ok(());
    };

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&flow);
    }

    println!("{} {} ({})", "Flow:".bold(), symbol.bold(), flow.period);
    print_flow_section("TOP BUYERS", &flow.top_buyers);
    print_flow_section("TOP SELLERS", &flow.top_sellers);
    print_flow_section("NEW POSITIONS", &flow.new_positions);
    print_flow_section("EXITED", &flow.exited);
    Ok(())
}

fn print_flow_section(title: &str, rows: &[HolderRow]) {
    println!("\n{}", title.bold());
    if rows.is_empty() {
        println!("-");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["RANK", "INVESTOR", "SHARES", "%", "SIGNAL"]);

    for (i, row) in rows.iter().enumerate() {
        table.add_row(vec![
            Cell::new(i + 1),
            Cell::new(&row.name),
            Cell::new(format_idr(row.shares)),
            Cell::new(format_bps(row.percentage_bps)),
            Cell::new(format_signal(row.signal)),
        ]);
    }
    println!("{table}");
}

fn handle_graph(args: &GraphArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let (nodes, edges) = graph::query_ownership_graph(&conn, &args.root, args.depth)?;

    if matches!(config.output, OutputFormat::Json) {
        #[derive(Serialize)]
        struct GraphOutput<'a> {
            nodes: &'a [crate::ownership::types::GraphNode],
            edges: &'a [crate::ownership::types::GraphEdge],
        }
        return json::print_json(&GraphOutput {
            nodes: &nodes,
            edges: &edges,
        });
    }

    match args.format.trim().to_lowercase().as_str() {
        "table" => {
            print!("{}", graph::format_graph_text(&nodes, &edges));
            Ok(())
        }
        "dot" => {
            print!("{}", graph::format_graph_dot(&nodes, &edges));
            Ok(())
        }
        other => Err(IdxError::ParseError(format!(
            "invalid --format '{other}', expected table|dot"
        ))),
    }
}

fn handle_changes(args: &ChangesArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let changes = db::query_changes(&conn, &args.from, &args.to)?;

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&changes);
    }

    if changes.is_empty() {
        println!(
            "No changes found between {} and {}.",
            args.from.trim(),
            args.to.trim()
        );
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["TICKER", "ENTITY", "CHANGE", "OLD%", "NEW%", "DELTA%"]);

    for row in changes {
        table.add_row(vec![
            Cell::new(row.ticker_code),
            Cell::new(row.entity_name),
            Cell::new(format_change_type(row.change_type)),
            Cell::new(
                row.old_bps
                    .map(format_bps)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            Cell::new(
                row.new_bps
                    .map(format_bps)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            Cell::new(format_signed_bps(row.delta_bps)),
        ]);
    }

    println!("{table}");
    Ok(())
}

fn handle_resolve(args: &ResolveArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;

    match &args.command {
        ResolveCommand::Unresolved { limit } => {
            let rows = db::list_unresolved(&conn, *limit)?;

            if matches!(config.output, OutputFormat::Json) {
                return json::print_json(&rows);
            }

            if rows.is_empty() {
                println!("No unresolved or low-confidence aliases found.");
                return Ok(());
            }

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    "RAW_NAME",
                    "SOURCE",
                    "TICKER",
                    "CURRENT_ENTITY",
                    "CONFIDENCE",
                ]);

            for row in rows {
                table.add_row(vec![
                    Cell::new(row.raw_name),
                    Cell::new(row.source),
                    Cell::new(row.ticker_code),
                    Cell::new(row.current_entity.unwrap_or_else(|| "-".to_string())),
                    Cell::new(
                        row.confidence
                            .map(|v| format!("{v:.2}"))
                            .unwrap_or_else(|| "-".to_string()),
                    ),
                ]);
            }
            println!("{table}");
            Ok(())
        }
        ResolveCommand::Map { alias, entity } => {
            db::manual_map(&conn, alias, entity)?;
            println!("Mapped alias '{alias}' -> '{entity}'.");
            Ok(())
        }
        ResolveCommand::Merge { keep, merge } => {
            let alias_updates: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM entity_aliases WHERE entity_id = ?1",
                    rusqlite::params![merge],
                    |row| row.get(0),
                )
                .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
            let ksei_updates: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM ksei_holdings WHERE entity_id = ?1",
                    rusqlite::params![merge],
                    |row| row.get(0),
                )
                .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
            let bing_updates: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM bing_holdings WHERE entity_id = ?1",
                    rusqlite::params![merge],
                    |row| row.get(0),
                )
                .map_err(|e| IdxError::DatabaseError(e.to_string()))?;

            db::merge_entities(&conn, *keep, *merge)?;

            println!(
                "Merged entity {merge} into {keep} (aliases: {alias_updates}, ksei_holdings: {ksei_updates}, bing_holdings: {bing_updates})."
            );
            Ok(())
        }
    }
}

fn handle_import(args: &ImportArgs, config: &IdxConfig) -> Result<(), IdxError> {
    if args.file.is_none() && args.url.is_none() && args.fetch_bing.is_none() {
        return Err(IdxError::ParseError(
            "provide one of: --file, --url, or --fetch-bing".to_string(),
        ));
    }

    if args.file.is_some() && args.url.is_some() {
        return Err(IdxError::ParseError(
            "--file and --url are mutually exclusive".to_string(),
        ));
    }

    if let Some(symbols) = &args.fetch_bing {
        let clean: Vec<String> = symbols
            .iter()
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_uppercase())
            .collect();

        if !clean.is_empty() {
            eprintln!(
                "info: --fetch-bing requested for {} symbol(s), implementation deferred for Sprint 6",
                clean.len()
            );
            for symbol in &clean {
                eprintln!("  - {symbol}");
            }

            return Err(IdxError::Unsupported(
                "--fetch-bing import is not implemented yet".to_string(),
            ));
        }
    }

    let Some(pdf_input) = resolve_pdf_input(args)? else {
        return Ok(());
    };

    let conn = db::open_db(config)?;

    let sha256 = sha256_file(&pdf_input.pdf_path)?;
    if !args.force && db::release_exists(&conn, &sha256)? {
        println!("Release already imported (sha256: {sha256}). Use --force to re-import.");
        return Ok(());
    }

    let raw_rows = parser::parse_ksei_pdf(&pdf_input.pdf_path)?;
    if raw_rows.is_empty() {
        return Err(IdxError::ParseError(
            "no KSEI rows parsed from PDF".to_string(),
        ));
    }

    let mut holdings = Vec::with_capacity(raw_rows.len());
    let mut ticker_ids = HashSet::new();

    for raw in &raw_rows {
        let draft = entities::normalize_ksei_row(raw)?;
        let ticker_id = db::upsert_ticker(&conn, &draft.ticker_code, draft.issuer_name.as_deref())?;
        let entity_id =
            entities::resolve_entity(&conn, &draft.raw_investor_name, OwnershipSource::Ksei)?;

        ticker_ids.insert(ticker_id);
        holdings.push(KseiHolding {
            id: 0,
            ticker_id,
            entity_id: Some(entity_id),
            raw_investor_name: draft.raw_investor_name,
            investor_type: draft.investor_type,
            locality: draft.locality,
            nationality: draft.nationality,
            domicile: draft.domicile,
            holdings_scripless: draft.holdings_scripless,
            holdings_scrip: draft.holdings_scrip,
            total_shares: draft.total_shares,
            percentage_bps: draft.percentage_bps,
            report_date: draft.report_date,
            release_sha256: sha256.clone(),
        });
    }

    let inserted_rows = db::insert_ksei_holdings(&conn, &holdings)?;
    let as_of_date = holdings
        .iter()
        .map(|h| h.report_date)
        .max()
        .ok_or_else(|| {
            IdxError::ParseError("missing report_date in parsed holdings".to_string())
        })?;

    let release = OwnershipRelease {
        id: 0,
        source_url: pdf_input.source_url,
        sha256,
        as_of_date,
        row_count: inserted_rows,
        imported_at: Utc::now().timestamp(),
    };
    let _ = db::insert_release(&conn, &release)?;

    println!(
        "Imported {} rows for {} tickers (as of {}).",
        inserted_rows,
        ticker_ids.len(),
        as_of_date.format("%Y-%m-%d")
    );

    Ok(())
}

fn handle_releases(config: &IdxConfig) -> Result<(), IdxError> {
    let conn = db::open_db(config)?;
    let releases = db::query_releases(&conn)?;

    if matches!(config.output, OutputFormat::Json) {
        return json::print_json(&releases);
    }

    if releases.is_empty() {
        println!("No ownership releases imported yet.");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["AS_OF", "ROWS", "SHA256", "SOURCE", "IMPORTED_AT"]);

    for r in releases {
        table.add_row(vec![
            Cell::new(r.as_of_date.format("%Y-%m-%d").to_string()),
            Cell::new(r.row_count),
            Cell::new(r.sha256),
            Cell::new(r.source_url.unwrap_or_else(|| "-".to_string())),
            Cell::new(r.imported_at),
        ]);
    }

    println!("{table}");
    Ok(())
}

fn parse_discovery_family(raw: &str) -> Result<Option<remote::OwnershipReportFamily>, IdxError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "all" => Ok(None),
        "above1" | "above-1" | "above_1" => {
            Ok(Some(remote::OwnershipReportFamily::AboveOnePercent))
        }
        "above5" | "above-5" | "above_5" => {
            Ok(Some(remote::OwnershipReportFamily::AboveFivePercent))
        }
        "investor-type" | "investor_type" | "investortype" => {
            Ok(Some(remote::OwnershipReportFamily::InvestorTypeBreakdown))
        }
        _ => Err(IdxError::ParseError(
            "invalid --family, expected: all|above1|above5|investor-type".to_string(),
        )),
    }
}

struct ResolvedPdfInput {
    pdf_path: PathBuf,
    source_url: Option<String>,
}

fn resolve_pdf_input(args: &ImportArgs) -> Result<Option<ResolvedPdfInput>, IdxError> {
    if let Some(path) = &args.file {
        if !path.exists() {
            return Err(IdxError::Io(format!(
                "input PDF not found: {}",
                path.display()
            )));
        }

        return Ok(Some(ResolvedPdfInput {
            pdf_path: path.clone(),
            source_url: None,
        }));
    }

    if let Some(url) = &args.url {
        let trimmed = url.trim();
        validate_import_url(trimmed)?;
        let target = cache_pdf_path(trimmed)?;
        download_pdf(trimmed, &target)?;
        return Ok(Some(ResolvedPdfInput {
            pdf_path: target,
            source_url: Some(trimmed.to_string()),
        }));
    }

    Ok(None)
}

fn cache_pdf_path(url: &str) -> Result<PathBuf, IdxError> {
    let dirs = ProjectDirs::from("", "", "idx")
        .ok_or_else(|| IdxError::Io("unable to resolve cache directory".to_string()))?;
    let raw_dir = dirs.cache_dir().join("ownership").join("raw");
    fs::create_dir_all(&raw_dir).map_err(|e| IdxError::Io(e.to_string()))?;

    let mut file_name = url
        .rsplit('/')
        .next()
        .unwrap_or("ksei-ownership.pdf")
        .split('?')
        .next()
        .unwrap_or("ksei-ownership.pdf")
        .to_string();

    if file_name.is_empty() || file_name == "/" {
        file_name = format!("ksei-{}.pdf", Utc::now().timestamp());
    }
    if !file_name.to_ascii_lowercase().ends_with(".pdf") {
        file_name.push_str(".pdf");
    }

    Ok(raw_dir.join(file_name))
}

fn validate_import_url(url: &str) -> Result<(), IdxError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(IdxError::InvalidInput(
            "ownership import --url accepts direct PDF URLs only".to_string(),
        ));
    }

    let normalized = trimmed.to_ascii_lowercase();
    let normalized = normalized
        .split(['?', '#'])
        .next()
        .unwrap_or(normalized.as_str());
    if normalized.ends_with(".pdf") {
        return Ok(());
    }

    Err(IdxError::InvalidInput(
        "ownership import --url accepts direct PDF URLs only; run `idx ownership discover` first to find the current supported attachment".to_string(),
    ))
}

fn download_pdf(url: &str, target: &Path) -> Result<(), IdxError> {
    if is_idx_url(url) {
        return remote::download_idx_pdf(url, target);
    }

    let response = ureq::get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
        )
        .header("Accept", "application/pdf,application/octet-stream,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Referer", "https://www.idx.co.id/")
        .call()
        .map_err(|e| IdxError::Http(format!("failed to download PDF: {e}")))?;

    let mut body = response.into_body();
    let bytes = body
        .read_to_vec()
        .map_err(|e| IdxError::Http(format!("failed reading PDF body: {e}")))?;
    remote::validate_pdf_payload(&bytes)?;

    fs::write(target, &bytes).map_err(|e| {
        IdxError::Io(format!(
            "failed writing cached PDF {}: {e}",
            target.display()
        ))
    })?;

    Ok(())
}

fn is_idx_url(url: &str) -> bool {
    let normalized = url.trim().to_ascii_lowercase();
    normalized.starts_with("https://www.idx.co.id/")
        || normalized.starts_with("http://www.idx.co.id/")
        || normalized.starts_with("https://idx.co.id/")
        || normalized.starts_with("http://idx.co.id/")
}

fn sha256_file(path: &Path) -> Result<String, IdxError> {
    let bytes = fs::read(path).map_err(|e| {
        IdxError::Io(format!(
            "failed to read file for sha256 {}: {e}",
            path.display()
        ))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn format_bps(bps: i64) -> String {
    format!("{:.2}%", bps as f64 / 100.0)
}

fn format_signal(signal: Option<FlowSignal>) -> String {
    match signal {
        Some(FlowSignal::Buyer) => "BUYER".green().to_string(),
        Some(FlowSignal::Seller) => "SELLER".red().to_string(),
        Some(FlowSignal::NewPosition) => "NEW".blue().to_string(),
        Some(FlowSignal::Exited) => "EXITED".yellow().to_string(),
        Some(FlowSignal::Holder) => "HOLDER".to_string(),
        None => "-".to_string(),
    }
}

fn format_change_type(change_type: ChangeType) -> String {
    match change_type {
        ChangeType::New => "NEW".green().to_string(),
        ChangeType::Exited => "EXITED".red().to_string(),
        ChangeType::Increased => "INCREASED".green().to_string(),
        ChangeType::Decreased => "DECREASED".red().to_string(),
    }
}

fn format_signed_bps(bps: i64) -> String {
    let pct = bps as f64 / 100.0;
    if bps > 0 {
        format!("+{pct:.2}%")
    } else {
        format!("{pct:.2}%")
    }
}
