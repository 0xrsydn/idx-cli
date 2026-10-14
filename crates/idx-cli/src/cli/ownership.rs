use std::path::PathBuf;

use clap::{Args, Subcommand};
use comfy_table::{Cell, ContentArrangement, Table, presets::UTF8_FULL};
use serde::Serialize;

use crate::config::IdxConfig;
use crate::output::json;
use crate::output::style::Paint;
use crate::output::table::format_idr;
use idx_core::error::IdxError;
use idx_core::investor::{
    ConcentrationRank, HolderSource, ImportSource, Investor, parse_report_family,
};
use idx_core::ownership::graph;
use idx_core::ownership::snapshot;
use idx_core::ownership::types::{ChangeType, FlowSignal, HolderRow, OwnershipSource};

#[derive(Debug, Args)]
#[command(
    about = "Ownership intelligence (KSEI + Bing)",
    long_about = "Ownership intelligence (KSEI + Bing).\n\nPreferred bootstrap path:\n  1. Run `idx ownership sync` to install or refresh a maintained SQLite snapshot.\n  2. If no snapshot manifest is available, run `idx ownership discover` and then `idx ownership import --url <pdf-url>`.\n  3. Local `.xlsx`, `.pdf`, `.zip`, and `.txt` imports remain available for manual or fallback workflows.",
    after_help = "Examples:\n  idx ownership sync\n  idx ownership sync --manifest /path/to/ownership-snapshot-manifest.json\n  idx ownership discover --limit 1\n  idx ownership import --url <pdf-url>\n  idx ownership releases"
)]
pub struct OwnershipCmd {
    #[command(subcommand)]
    pub command: OwnershipCommand,
}

#[derive(Debug, Subcommand)]
pub enum OwnershipCommand {
    #[command(
        about = "Discover the latest IDX-hosted ownership report URLs",
        after_help = "Examples:\n  idx ownership discover --limit 1\n  idx ownership discover --family all --limit 6\n\nUse this when no snapshot manifest is available and you need a direct XLSX or PDF URL for `idx ownership import --url`."
    )]
    Discover(DiscoverArgs),
    #[command(
        about = "Import ownership data directly from source files",
        long_about = "Import ownership data directly from source files.\n\nPrefer `idx ownership sync` for normal bootstrap/update flows. Use `import` when you need a direct PDF import from IDX discovery output, a local PDF, or a local KSEI archive fallback file.",
        after_help = "Examples:\n  idx ownership import --url <pdf-url-from-discover>\n  idx ownership import --file /path/to/peng-2026-08-00017-satu-persen.xlsx\n  idx ownership import --file /path/to/ksei.pdf\n  idx ownership import --file /path/to/BalanceposEfek20260227.zip"
    )]
    Import(ImportArgs),
    #[command(
        about = "Install or refresh a maintained ownership SQLite snapshot",
        long_about = "Install or refresh a maintained ownership SQLite snapshot.\n\nThis is the normal bootstrap/update path for ownership data.\n\nManifest lookup order:\n  1. `--manifest`\n  2. `IDX_OWNERSHIP_SNAPSHOT_MANIFEST`\n  3. `ownership.snapshot_manifest` in config",
        after_help = "Examples:\n  idx ownership sync\n  idx ownership sync --manifest /path/to/ownership-snapshot-manifest.json\n  IDX_OWNERSHIP_SNAPSHOT_MANIFEST=https://example.com/latest.json idx ownership sync\n\nIf none of the above are set, `ownership sync` falls back to the built-in published manifest URL."
    )]
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
    /// Direct URL to a remote above-1% XLSX or PDF, typically from `ownership discover`.
    #[arg(long)]
    pub url: Option<String>,
    /// Path to a local ownership file: above-1% XLSX or PDF (primary), ZIP/TXT archive (fallback).
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// Fetch Bing institutional data for these symbols.
    #[arg(long, value_delimiter = ',')]
    pub fetch_bing: Option<Vec<String>>,
    /// Re-import even if the release SHA-256 has already been seen.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Snapshot manifest location (URL or local path). If omitted, idx falls back through env/config and then the published default URL.
    #[arg(long)]
    pub manifest: Option<String>,
    /// Replace the local DB even when it is already current or newer than the snapshot.
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
    let investor = Investor::new(&config.core);
    let inv = &investor;
    match cmd {
        OwnershipCommand::Discover(args) => handle_discover(inv, args, config),
        OwnershipCommand::Import(args) => handle_import(inv, args, config),
        OwnershipCommand::Sync(args) => handle_sync(inv, args, config),
        OwnershipCommand::Ticker(args) => handle_ticker(inv, args, config),
        OwnershipCommand::Entity(args) => handle_entity(inv, args, config),
        OwnershipCommand::Search(args) => handle_search(inv, args, config),
        OwnershipCommand::CrossHolders(args) => handle_cross_holders(inv, args, config),
        OwnershipCommand::Concentration(args) => handle_concentration(inv, args, config),
        OwnershipCommand::Flow(args) => handle_flow(inv, args, config),
        OwnershipCommand::Changes(args) => handle_changes(inv, args, config),
        OwnershipCommand::Graph(args) => handle_graph(inv, args, config),
        OwnershipCommand::Resolve(args) => handle_resolve(inv, args, config),
        OwnershipCommand::Releases => handle_releases(inv, config),
    }
}

fn handle_sync(investor: &Investor, args: &SyncArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let result = investor.sync(args.manifest.as_deref(), args.force)?;

    if config.output.is_json() {
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

fn handle_discover(
    investor: &Investor,
    args: &DiscoverArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
    if args.limit == 0 {
        return Err(IdxError::ParseError(
            "--limit must be greater than 0".to_string(),
        ));
    }

    let family = parse_report_family(&args.family)?;
    let reports = investor.discover(family, args.limit)?;

    if config.output.is_json() {
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

fn handle_ticker(
    investor: &Investor,
    args: &TickerArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
    let source: HolderSource = args.source.parse()?;
    let data = investor.ticker(&args.symbol, source)?;

    if config.output.is_json() {
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
                Some(idx_core::ownership::types::Locality::Local) => "L",
                Some(idx_core::ownership::types::Locality::Foreign) => "F",
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

fn handle_entity(
    investor: &Investor,
    args: &EntityArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
    let data = investor.entity(&args.name)?;

    if config.output.is_json() {
        return json::print_json(&data);
    }

    println!(
        "{} {}",
        "Entity:".bold(),
        data.entity.canonical_name.as_str().bold()
    );
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

fn handle_search(
    investor: &Investor,
    args: &SearchArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
    let entities = investor.search(&args.query, args.limit)?;

    if config.output.is_json() {
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

fn handle_cross_holders(
    investor: &Investor,
    args: &CrossHolderArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
    let rows = investor.cross_holders(args.min_tickers, args.top)?;

    if config.output.is_json() {
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

fn handle_concentration(
    investor: &Investor,
    args: &ConcentrationArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
    let by: ConcentrationRank = args.by.parse()?;
    let rows = investor.concentration(by, args.top)?;

    if config.output.is_json() {
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

fn handle_flow(investor: &Investor, args: &FlowArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let symbol = args.symbol.trim().to_uppercase();
    let flow = investor.flow(&symbol)?;
    let Some(flow) = flow else {
        if config.output.is_json() {
            return json::print_json(
                &Option::<idx_core::ownership::types::InstitutionalFlow>::None,
            );
        }
        println!("No institutional flow data available for {symbol}.");
        return Ok(());
    };

    if config.output.is_json() {
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

fn handle_graph(investor: &Investor, args: &GraphArgs, config: &IdxConfig) -> Result<(), IdxError> {
    let (nodes, edges) = investor.graph(&args.root, args.depth)?;

    if config.output.is_json() {
        #[derive(Serialize)]
        struct GraphOutput<'a> {
            nodes: &'a [idx_core::ownership::types::GraphNode],
            edges: &'a [idx_core::ownership::types::GraphEdge],
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

fn handle_changes(
    investor: &Investor,
    args: &ChangesArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
    let changes = investor.changes(&args.from, &args.to, None)?;

    if config.output.is_json() {
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

fn handle_resolve(
    investor: &Investor,
    args: &ResolveArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
    match &args.command {
        ResolveCommand::Unresolved { limit } => {
            let rows = investor.unresolved(*limit)?;

            if config.output.is_json() {
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
            investor.map_alias(alias, entity)?;
            if config.output.is_json() {
                return json::print_json(&serde_json::json!({
                    "action": "mapped",
                    "alias": alias,
                    "entity": entity,
                }));
            }
            println!("Mapped alias '{alias}' -> '{entity}'.");
            Ok(())
        }
        ResolveCommand::Merge { keep, merge } => {
            let merged = investor.merge_entities(*keep, *merge)?;
            let alias_updates = merged.alias_updates;
            let ksei_updates = merged.ksei_holdings_updates;
            let bing_updates = merged.bing_holdings_updates;

            if config.output.is_json() {
                return json::print_json(&serde_json::json!({
                    "action": "merged",
                    "keep": keep,
                    "merge": merge,
                    "alias_updates": alias_updates,
                    "ksei_holdings_updates": ksei_updates,
                    "bing_holdings_updates": bing_updates,
                }));
            }

            println!(
                "Merged entity {merge} into {keep} (aliases: {alias_updates}, ksei_holdings: {ksei_updates}, bing_holdings: {bing_updates})."
            );
            Ok(())
        }
    }
}

fn handle_import(
    investor: &Investor,
    args: &ImportArgs,
    config: &IdxConfig,
) -> Result<(), IdxError> {
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

    if args.fetch_bing.as_ref().is_some_and(|symbols| {
        symbols
            .iter()
            .flat_map(|value| value.split(','))
            .any(|value| !value.trim().is_empty())
    }) {
        return Err(IdxError::Unsupported(
            "--fetch-bing import is not implemented yet".to_string(),
        ));
    }

    let source = match (&args.file, &args.url) {
        (Some(path), _) => ImportSource::File(path.clone()),
        (None, Some(url)) => ImportSource::Url(url.clone()),
        // An empty --fetch-bing list has nothing to import.
        (None, None) => return Ok(()),
    };
    let result = investor.import(&source, args.force)?;

    if config.output.is_json() {
        return json::print_json(&result);
    }

    match result.as_of_date.as_deref() {
        None => println!(
            "Release already imported (sha256: {}). Use --force to re-import.",
            result.sha256
        ),
        Some(as_of_date) => println!(
            "Imported {} rows for {} tickers (as of {}).",
            result.inserted_rows, result.ticker_count, as_of_date
        ),
    }

    Ok(())
}

fn handle_releases(investor: &Investor, config: &IdxConfig) -> Result<(), IdxError> {
    let releases = investor.releases()?;

    if config.output.is_json() {
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
