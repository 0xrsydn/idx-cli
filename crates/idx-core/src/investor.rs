//! Investor (ownership) data: queries over the local SQLite database, plus
//! the import, sync, and discovery that populate it.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{NaiveDate, Utc};
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::CoreConfig;
use crate::error::IdxError;
use crate::ownership::remote::{self, DiscoveredOwnershipPdf, OwnershipReportFamily};
use crate::ownership::snapshot::{self, OwnershipSyncResult};
use crate::ownership::types::{
    ChangeRow, ConcentrationMetrics, CrossHolderRow, Entity, EntityHoldings, GraphEdge, GraphNode,
    InstitutionalFlow, KseiHolding, OwnershipRelease, OwnershipSource, TickerOwnership,
    UnresolvedRow,
};
use crate::ownership::{archive, db, entities, graph, parser, search, xlsx};

/// Ownership database access. Opening is lazy: each call opens (and, if
/// needed, creates and migrates) the database at the configured path.
pub struct Investor {
    db_path: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
}

impl Investor {
    pub fn new(config: &CoreConfig) -> Self {
        Self {
            db_path: config.ownership_db.clone(),
            cache_dir: config.cache_dir.clone(),
        }
    }

    /// The database path: `CoreConfig::ownership_db`, else `ownership.db_path`
    /// from `config.toml`, else `$XDG_DATA_HOME/idx/ownership.db`.
    pub fn db_path(&self) -> Result<PathBuf, IdxError> {
        match &self.db_path {
            Some(path) => Ok(path.clone()),
            None => db::db_path(),
        }
    }

    /// Cross-process lock beside the database. Writers (sync install, import,
    /// entity edits) hold it exclusively; readers hold it shared, so a
    /// snapshot swap never happens under an open connection. Released when
    /// the returned file is dropped.
    fn lock_file(&self, exclusive: bool) -> Result<fs::File, IdxError> {
        // Every path to one database (symlinks included) shares one lock.
        let real = db::resolve_db_path(&self.db_path()?)?;
        lock_at(&real, exclusive)
    }

    fn write_lock(&self) -> Result<fs::File, IdxError> {
        self.lock_file(true)
    }

    /// A read connection holding the shared lock for its lifetime.
    fn connect(&self) -> Result<ReadConnection, IdxError> {
        let real = db::resolve_db_path(&self.db_path()?)?;
        let lock = match lock_at(&real, false) {
            Ok(lock) => Some(lock),
            // No lock file and none can be created (a read-only filesystem):
            // no writer can lock it to replace the database either.
            Err(_) if unlockable(&real) => None,
            Err(err) => return Err(err),
        };
        Ok(ReadConnection {
            conn: self.open_db()?,
            _lock: lock,
        })
    }

    /// For writers, which already hold the exclusive lock.
    fn open_db(&self) -> Result<Connection, IdxError> {
        db::open_db_at(&self.db_path()?)
    }

    pub fn releases(&self) -> Result<Vec<OwnershipRelease>, IdxError> {
        db::query_releases(&*self.connect()?)
    }

    /// Whether the database has data, and which releases it holds.
    pub fn status(&self) -> Result<OwnershipStatus, IdxError> {
        let db_path = self.db_path()?;
        let releases = db::query_releases(&*self.connect()?)?;
        Ok(OwnershipStatus {
            db_path: db_path.display().to_string(),
            populated: !releases.is_empty(),
            latest_as_of_date: releases.iter().map(|r| r.as_of_date).max(),
            releases,
        })
    }

    /// Holders of `symbol` in the latest release, optionally limited to one
    /// source (ranks and concentration are recomputed for that source).
    pub fn ticker(&self, symbol: &str, source: HolderSource) -> Result<TickerOwnership, IdxError> {
        let conn = self.connect()?;
        let symbol = symbol.trim().to_uppercase();
        let mut data = db::query_ticker_holdings(&conn, &symbol)?;
        if data.holders.is_empty() {
            return Err(IdxError::ParseError(format!(
                "no ownership data found for symbol {symbol}"
            )));
        }

        if source != HolderSource::All {
            data.holders.retain(|h| match source {
                HolderSource::Ksei => matches!(h.source, OwnershipSource::Ksei),
                HolderSource::Bing => matches!(h.source, OwnershipSource::Bing),
                HolderSource::All => true,
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
        Ok(data)
    }

    /// Holdings of the entity whose name best matches `name` (shortest
    /// canonical name containing it).
    pub fn entity(&self, name: &str) -> Result<EntityHoldings, IdxError> {
        let conn = self.connect()?;
        let pattern = format!("%{}%", name.trim());
        let entity_id = conn
            .query_row(
                "SELECT id FROM entities WHERE canonical_name LIKE ?1 COLLATE NOCASE ORDER BY LENGTH(canonical_name) ASC LIMIT 1",
                rusqlite::params![pattern],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| IdxError::ParseError(format!("entity not found: {name}")))?;
        db::query_entity_holdings(&conn, entity_id)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Entity>, IdxError> {
        search::fts_search(&*self.connect()?, query, limit)
    }

    pub fn cross_holders(
        &self,
        min_tickers: usize,
        top: usize,
    ) -> Result<Vec<CrossHolderRow>, IdxError> {
        db::query_cross_holders(&*self.connect()?, min_tickers, top)
    }

    pub fn concentration(
        &self,
        by: ConcentrationRank,
        top: usize,
    ) -> Result<Vec<(String, ConcentrationMetrics)>, IdxError> {
        db::query_concentration(&*self.connect()?, by.as_str(), top)
    }

    /// Institutional (Bing) flow; `None` when no flow data exists.
    pub fn flow(&self, symbol: &str) -> Result<Option<InstitutionalFlow>, IdxError> {
        let conn = self.connect()?;
        let symbol = symbol.trim().to_uppercase();
        let ticker_id = db::get_ticker_id(&conn, &symbol)?
            .ok_or_else(|| IdxError::SymbolNotFound(symbol.clone()))?;
        db::query_bing_flow(&conn, ticker_id)
    }

    /// Holder changes between two release dates, optionally for one ticker.
    pub fn changes(
        &self,
        from: &str,
        to: &str,
        ticker: Option<&str>,
    ) -> Result<Vec<ChangeRow>, IdxError> {
        let mut rows = db::query_changes(&*self.connect()?, from, to)?;
        if let Some(ticker) = ticker {
            let ticker = ticker.trim().to_uppercase();
            rows.retain(|row| row.ticker_code == ticker);
        }
        Ok(rows)
    }

    pub fn graph(
        &self,
        root: &str,
        depth: usize,
    ) -> Result<(Vec<GraphNode>, Vec<GraphEdge>), IdxError> {
        graph::query_ownership_graph(&*self.connect()?, root, depth)
    }

    pub fn unresolved(&self, limit: usize) -> Result<Vec<UnresolvedRow>, IdxError> {
        db::list_unresolved(&*self.connect()?, limit)
    }

    pub fn map_alias(&self, alias: &str, entity: &str) -> Result<(), IdxError> {
        let _lock = self.write_lock()?;
        db::manual_map(&self.open_db()?, alias, entity)
    }

    /// Merges entity `merge` into `keep`, reporting how many rows moved.
    pub fn merge_entities(&self, keep: i64, merge: i64) -> Result<MergeResult, IdxError> {
        let _lock = self.write_lock()?;
        let conn = self.open_db()?;
        let count = |table: &str| -> Result<i64, IdxError> {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE entity_id = ?1"),
                rusqlite::params![merge],
                |row| row.get(0),
            )
            .map_err(|e| IdxError::DatabaseError(e.to_string()))
        };
        let alias_updates = count("entity_aliases")?;
        let ksei_holdings_updates = count("ksei_holdings")?;
        let bing_holdings_updates = count("bing_holdings")?;
        db::merge_entities(&conn, keep, merge)?;
        Ok(MergeResult {
            keep,
            merge,
            alias_updates,
            ksei_holdings_updates,
            bing_holdings_updates,
        })
    }

    /// Lists IDX ownership reports (newest first), live from IDX.
    pub fn discover(
        &self,
        family: Option<OwnershipReportFamily>,
        limit: usize,
    ) -> Result<Vec<DiscoveredOwnershipPdf>, IdxError> {
        remote::discover_idx_ownership_reports(family, limit)
    }

    /// Installs or refreshes the maintained snapshot. `manifest` overrides
    /// the configured manifest location.
    pub fn sync(
        &self,
        manifest: Option<&str>,
        force: bool,
    ) -> Result<OwnershipSyncResult, IdxError> {
        let manifest_source = snapshot::resolve_manifest_source(manifest)?;
        snapshot::sync_snapshot(&manifest_source, &self.db_path()?, force, lock_at)
    }

    /// Imports one report. A release already imported (same SHA-256) is
    /// skipped unless `force`, which replaces it.
    pub fn import(&self, source: &ImportSource, force: bool) -> Result<ImportResult, IdxError> {
        // Validate and download first (unchanged error order), then lock
        // only around the database writes.
        let input = self.resolve_import_input(source)?;
        let _lock = self.write_lock()?;
        let conn = self.open_db()?;

        let sha256 = sha256_file(&input.path)?;
        if !force && db::release_exists(&conn, &sha256)? {
            return Ok(ImportResult {
                action: "skipped_existing",
                inserted_rows: 0,
                ticker_count: 0,
                as_of_date: None,
                sha256,
                source_url: input.source_url,
            });
        }

        let drafts = match input.format {
            ImportFormat::Pdf => {
                let raw_rows = parser::parse_ksei_pdf(&input.path)?;
                if raw_rows.is_empty() {
                    return Err(IdxError::ParseError(
                        "no KSEI rows parsed from PDF".to_string(),
                    ));
                }
                let mut drafts = Vec::with_capacity(raw_rows.len());
                for raw in &raw_rows {
                    drafts.push(entities::normalize_ksei_row(raw)?);
                }
                drafts
            }
            ImportFormat::Archive => archive::parse_balancepos_file(&input.path)?,
            ImportFormat::Xlsx => xlsx::parse_above1_xlsx_file(&input.path)?,
        };

        let mut holdings = Vec::with_capacity(drafts.len());
        let mut ticker_ids = HashSet::new();
        for draft in drafts {
            let ticker_id =
                db::upsert_ticker(&conn, &draft.ticker_code, draft.issuer_name.as_deref())?;
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

        let as_of_date = holdings
            .iter()
            .map(|h| h.report_date)
            .max()
            .ok_or_else(|| {
                IdxError::ParseError("missing report_date in parsed holdings".to_string())
            })?;

        let release = OwnershipRelease {
            id: 0,
            source_url: input.source_url,
            sha256,
            as_of_date,
            row_count: holdings.len(),
            imported_at: Utc::now().timestamp(),
        };
        let inserted_rows = db::write_ksei_release(&conn, &release, &holdings, force)?;

        Ok(ImportResult {
            action: if force { "reimported" } else { "imported" },
            inserted_rows,
            ticker_count: ticker_ids.len(),
            as_of_date: Some(as_of_date.format("%Y-%m-%d").to_string()),
            sha256: release.sha256,
            source_url: release.source_url,
        })
    }

    fn resolve_import_input(&self, source: &ImportSource) -> Result<ImportInput, IdxError> {
        match source {
            ImportSource::File(path) => {
                if !path.exists() {
                    return Err(IdxError::Io(format!(
                        "input ownership file not found: {}",
                        path.display()
                    )));
                }
                Ok(ImportInput {
                    path: path.clone(),
                    source_url: None,
                    format: detect_local_import_format(path)?,
                })
            }
            ImportSource::Url(url) => {
                let trimmed = url.trim();
                let format = validate_import_url(trimmed)?;
                let target = self.download_path(trimmed, format)?;
                match format {
                    ImportFormat::Xlsx => download_xlsx(trimmed, &target)?,
                    _ => download_pdf(trimmed, &target)?,
                }
                Ok(ImportInput {
                    path: target,
                    source_url: Some(trimmed.to_string()),
                    format,
                })
            }
        }
    }

    fn download_path(&self, url: &str, format: ImportFormat) -> Result<PathBuf, IdxError> {
        let extension = match format {
            ImportFormat::Xlsx => ".xlsx",
            _ => ".pdf",
        };
        let cache_dir = match &self.cache_dir {
            Some(dir) => dir.clone(),
            None => crate::cache::cache_dir()?,
        };
        let raw_dir = cache_dir.join("ownership").join("raw");
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
            file_name = format!("ksei-{}{extension}", Utc::now().timestamp());
        }
        if !file_name.to_ascii_lowercase().ends_with(extension) {
            file_name.push_str(extension);
        }

        Ok(raw_dir.join(file_name))
    }
}

/// Where an import reads its report from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    /// Local `.xlsx`, `.pdf`, `.zip`, or `.txt`.
    File(PathBuf),
    /// Direct XLSX or PDF URL; IDX URLs use browser impersonation.
    Url(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    /// `imported`, `reimported`, or `skipped_existing`.
    pub action: &'static str,
    pub inserted_rows: usize,
    pub ticker_count: usize,
    pub as_of_date: Option<String>,
    pub sha256: String,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MergeResult {
    pub keep: i64,
    pub merge: i64,
    pub alias_updates: i64,
    pub ksei_holdings_updates: i64,
    pub bing_holdings_updates: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnershipStatus {
    pub db_path: String,
    /// False until `sync` or `import` has loaded at least one release.
    pub populated: bool,
    pub latest_as_of_date: Option<NaiveDate>,
    pub releases: Vec<OwnershipRelease>,
}

fn lock_path(db_path: &Path) -> PathBuf {
    let mut name = db_path.as_os_str().to_os_string();
    name.push(".lock");
    PathBuf::from(name)
}

/// Locks `<db>.lock` beside the resolved database path. A shared lock does
/// not need write access, so readers fall back to opening an existing lock
/// file read-only (e.g. one created by another user).
fn lock_at(db_path: &Path, exclusive: bool) -> Result<fs::File, IdxError> {
    let path = lock_path(db_path);
    let opened = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path);
    let file = match opened {
        Ok(file) => file,
        Err(_) if !exclusive && path.exists() => fs::File::open(&path)
            .map_err(|e| IdxError::Io(format!("failed to open ownership lock: {e}")))?,
        Err(e) => return Err(IdxError::Io(format!("failed to open ownership lock: {e}"))),
    };
    if exclusive {
        file.lock()
    } else {
        file.lock_shared()
    }
    .map_err(|e| IdxError::Io(format!("failed to lock ownership database: {e}")))?;
    Ok(file)
}

/// True when the lock file is absent and cannot be created.
fn unlockable(db_path: &Path) -> bool {
    let path = lock_path(db_path);
    !path.exists()
        && matches!(
            fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem)
        )
}

struct ReadConnection {
    // Dropped (closed) before the lock it is declared ahead of.
    conn: Connection,
    _lock: Option<fs::File>,
}

impl std::ops::Deref for ReadConnection {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.conn
    }
}

/// Holder sources for `Investor::ticker`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderSource {
    All,
    Ksei,
    Bing,
}

impl fmt::Display for HolderSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::All => "all",
            Self::Ksei => "ksei",
            Self::Bing => "bing",
        })
    }
}

impl FromStr for HolderSource {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, IdxError> {
        match value.trim().to_lowercase().as_str() {
            "all" => Ok(Self::All),
            "ksei" => Ok(Self::Ksei),
            "bing" => Ok(Self::Bing),
            _ => Err(IdxError::ParseError(
                "invalid --source, expected: all|ksei|bing".to_string(),
            )),
        }
    }
}

/// Ranking metric for `Investor::concentration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConcentrationRank {
    Top1,
    Top3,
    Hhi,
}

impl ConcentrationRank {
    fn as_str(self) -> &'static str {
        match self {
            Self::Top1 => "top1",
            Self::Top3 => "top3",
            Self::Hhi => "hhi",
        }
    }
}

impl FromStr for ConcentrationRank {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, IdxError> {
        match value.trim().to_lowercase().as_str() {
            "top1" => Ok(Self::Top1),
            "top3" => Ok(Self::Top3),
            "hhi" => Ok(Self::Hhi),
            _ => Err(IdxError::ParseError(
                "invalid --by, expected: top1|top3|hhi".to_string(),
            )),
        }
    }
}

/// `all` (None) or one report family, with the aliases the CLI accepts.
pub fn parse_report_family(raw: &str) -> Result<Option<OwnershipReportFamily>, IdxError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "all" => Ok(None),
        "above1" | "above-1" | "above_1" => Ok(Some(OwnershipReportFamily::AboveOnePercent)),
        "above5" | "above-5" | "above_5" => Ok(Some(OwnershipReportFamily::AboveFivePercent)),
        "investor-type" | "investor_type" | "investortype" => {
            Ok(Some(OwnershipReportFamily::InvestorTypeBreakdown))
        }
        _ => Err(IdxError::ParseError(
            "invalid --family, expected: all|above1|above5|investor-type".to_string(),
        )),
    }
}

struct ImportInput {
    path: PathBuf,
    source_url: Option<String>,
    format: ImportFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportFormat {
    Pdf,
    Archive,
    Xlsx,
}

fn detect_local_import_format(path: &Path) -> Result<ImportFormat, IdxError> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("pdf") => Ok(ImportFormat::Pdf),
        Some("xlsx") if xlsx::supports_xlsx_file(path) => Ok(ImportFormat::Xlsx),
        Some("zip") | Some("txt") if archive::supports_local_archive_file(path) => {
            Ok(ImportFormat::Archive)
        }
        _ => Err(IdxError::InvalidInput(format!(
            "unsupported local ownership file {}; expected a .xlsx, .pdf, .zip, or .txt input",
            path.display()
        ))),
    }
}

fn validate_import_url(url: &str) -> Result<ImportFormat, IdxError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(IdxError::InvalidInput(
            "ownership import --url accepts direct XLSX or PDF URLs only".to_string(),
        ));
    }

    let normalized = trimmed.to_ascii_lowercase();
    let normalized = normalized
        .split(['?', '#'])
        .next()
        .unwrap_or(normalized.as_str());
    if normalized.ends_with(".pdf") {
        return Ok(ImportFormat::Pdf);
    }
    if normalized.ends_with(".xlsx") {
        return Ok(ImportFormat::Xlsx);
    }

    Err(IdxError::InvalidInput(
        "ownership import --url accepts direct XLSX or PDF URLs only; run `idx ownership discover` first to find the current supported file".to_string(),
    ))
}

/// Above ureq's 10 MiB default: the monthly holder-register PDF can grow past it.
const REMOTE_REPORT_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

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

    let bytes = response
        .into_body()
        .with_config()
        .limit(REMOTE_REPORT_LIMIT_BYTES)
        .read_to_vec()
        .map_err(|e| IdxError::Http(format!("failed reading PDF body: {e}")))?;
    remote::validate_pdf_payload(&bytes)?;

    fs::write(target, &bytes).map_err(|e| {
        IdxError::Io(format!(
            "failed writing cached PDF {}: {e}",
            target.display()
        ))
    })
}

fn download_xlsx(url: &str, target: &Path) -> Result<(), IdxError> {
    if is_idx_url(url) {
        return remote::download_idx_xlsx(url, target);
    }

    let response = ureq::get(url)
        .header(
            "Accept",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet,application/octet-stream,*/*;q=0.8",
        )
        .call()
        .map_err(|e| IdxError::Http(format!("failed to download XLSX: {e}")))?;
    let bytes = response
        .into_body()
        .with_config()
        .limit(REMOTE_REPORT_LIMIT_BYTES)
        .read_to_vec()
        .map_err(|e| IdxError::Http(format!("failed reading XLSX body: {e}")))?;
    remote::validate_xlsx_payload(&bytes)?;

    fs::write(target, &bytes).map_err(|e| {
        IdxError::Io(format!(
            "failed writing cached XLSX {}: {e}",
            target.display()
        ))
    })
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

#[cfg(test)]
mod tests {
    use super::{ConcentrationRank, HolderSource, parse_report_family};
    use crate::ownership::remote::OwnershipReportFamily;

    #[test]
    fn parses_holder_sources_and_ranks() {
        assert_eq!(
            " KSEI ".parse::<HolderSource>().unwrap(),
            HolderSource::Ksei
        );
        assert!("x".parse::<HolderSource>().is_err());
        assert_eq!(
            "HHI".parse::<ConcentrationRank>().unwrap(),
            ConcentrationRank::Hhi
        );
        assert!("top2".parse::<ConcentrationRank>().is_err());
    }

    #[test]
    fn parses_report_family_aliases() {
        assert_eq!(parse_report_family("all").unwrap(), None);
        assert_eq!(
            parse_report_family("above_1").unwrap(),
            Some(OwnershipReportFamily::AboveOnePercent)
        );
        assert!(parse_report_family("above10").is_err());
    }
}
