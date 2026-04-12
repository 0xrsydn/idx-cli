use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::get_config_value;
use crate::error::IdxError;
use crate::ownership::db;
use crate::ownership::types::OwnershipRelease;

pub const SNAPSHOT_MANIFEST_CONFIG_KEY: &str = "ownership.snapshot_manifest";
pub const SNAPSHOT_MANIFEST_ENV: &str = "IDX_OWNERSHIP_SNAPSHOT_MANIFEST";
pub const SNAPSHOT_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_SNAPSHOT_MANIFEST_URL: &str = "https://github.com/0xrsydn/idx-cli/releases/download/ownership-snapshot-current/ownership-snapshot-manifest.json";

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnershipSnapshotManifest {
    pub schema_version: u32,
    pub generated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<OwnershipSnapshotSource>,
    pub snapshot: OwnershipSnapshotArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnershipSnapshotSource {
    pub family: String,
    pub listing_page_url: String,
    pub query_url: String,
    pub pdf_url: String,
    pub title: String,
    pub publish_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnershipSnapshotArtifact {
    pub kind: String,
    pub compression: String,
    pub version: String,
    pub download_url: String,
    pub sqlite_sha256: String,
    pub size_bytes: u64,
    pub release_count: usize,
    pub latest_as_of_date: NaiveDate,
    pub latest_release_sha256: String,
    pub latest_row_count: usize,
    pub ticker_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipSyncAction {
    Installed,
    Updated,
    Refreshed,
    NoChange,
    SkippedNewer,
    SkippedDiverged,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OwnershipSyncResult {
    pub action: OwnershipSyncAction,
    pub manifest: String,
    pub db_path: String,
    pub snapshot_version: String,
    pub latest_as_of_date: NaiveDate,
    pub release_count: usize,
    pub ticker_count: usize,
    pub reason: String,
}

#[derive(Debug, Clone)]
struct LocalSnapshotState {
    latest_release: Option<OwnershipRelease>,
    release_count: usize,
}

#[derive(Debug, Clone)]
struct SyncDecision {
    action: OwnershipSyncAction,
    should_download: bool,
    reason: String,
}

pub fn resolve_manifest_source(explicit: Option<&str>) -> Result<String, IdxError> {
    let env_value = std::env::var(SNAPSHOT_MANIFEST_ENV).ok();
    let config_value = get_config_value(SNAPSHOT_MANIFEST_CONFIG_KEY)?;

    resolve_manifest_source_with(explicit, env_value.as_deref(), config_value.as_deref())
}

fn resolve_manifest_source_with(
    explicit: Option<&str>,
    env_value: Option<&str>,
    config_value: Option<&str>,
) -> Result<String, IdxError> {
    for value in [explicit, env_value, config_value].into_iter().flatten() {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    Ok(DEFAULT_SNAPSHOT_MANIFEST_URL.to_string())
}

pub fn fetch_manifest(source: &str) -> Result<OwnershipSnapshotManifest, IdxError> {
    let raw = read_text(source, "ownership snapshot manifest")?;
    parse_manifest(&raw)
}

pub fn parse_manifest(raw: &str) -> Result<OwnershipSnapshotManifest, IdxError> {
    let manifest: OwnershipSnapshotManifest = serde_json::from_str(raw).map_err(|e| {
        IdxError::ParseError(format!(
            "failed to parse ownership snapshot manifest JSON: {e}"
        ))
    })?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn sync_snapshot(
    manifest_source: &str,
    db_path: &Path,
    force: bool,
) -> Result<OwnershipSyncResult, IdxError> {
    let manifest = fetch_manifest(manifest_source)?;
    let local_state = inspect_local_db(db_path)?;
    let decision = build_sync_decision(local_state.as_ref(), &manifest.snapshot, force);

    if !decision.should_download {
        return Ok(OwnershipSyncResult {
            action: decision.action,
            manifest: manifest_source.to_string(),
            db_path: db_path.display().to_string(),
            snapshot_version: manifest.snapshot.version.clone(),
            latest_as_of_date: manifest.snapshot.latest_as_of_date,
            release_count: manifest.snapshot.release_count,
            ticker_count: manifest.snapshot.ticker_count,
            reason: decision.reason,
        });
    }

    let bytes = read_bytes(&manifest.snapshot.download_url, "ownership snapshot SQLite")?;
    validate_downloaded_bytes(&bytes, &manifest.snapshot)?;

    let temp_path = build_temp_path(db_path);
    if let Some(parent) = temp_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| {
            IdxError::Io(format!(
                "failed to create snapshot temp directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    fs::write(&temp_path, &bytes).map_err(|e| {
        IdxError::Io(format!(
            "failed to write downloaded snapshot {}: {e}",
            temp_path.display()
        ))
    })?;

    let install_result = (|| -> Result<(), IdxError> {
        validate_snapshot_db(&temp_path, &manifest.snapshot)?;
        install_snapshot_file(&temp_path, db_path)
    })();

    if install_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    install_result?;

    Ok(OwnershipSyncResult {
        action: decision.action,
        manifest: manifest_source.to_string(),
        db_path: db_path.display().to_string(),
        snapshot_version: manifest.snapshot.version.clone(),
        latest_as_of_date: manifest.snapshot.latest_as_of_date,
        release_count: manifest.snapshot.release_count,
        ticker_count: manifest.snapshot.ticker_count,
        reason: decision.reason,
    })
}

fn validate_manifest(manifest: &OwnershipSnapshotManifest) -> Result<(), IdxError> {
    if manifest.schema_version != SNAPSHOT_MANIFEST_SCHEMA_VERSION {
        return Err(IdxError::ParseError(format!(
            "unsupported ownership snapshot manifest schema_version {}; expected {}",
            manifest.schema_version, SNAPSHOT_MANIFEST_SCHEMA_VERSION
        )));
    }

    if manifest.generated_at.trim().is_empty() {
        return Err(IdxError::ParseError(
            "ownership snapshot manifest is missing generated_at".to_string(),
        ));
    }

    if let Some(source) = &manifest.source {
        if source.family.trim().is_empty() {
            return Err(IdxError::ParseError(
                "ownership snapshot manifest is missing source.family".to_string(),
            ));
        }
        if source.listing_page_url.trim().is_empty() {
            return Err(IdxError::ParseError(
                "ownership snapshot manifest is missing source.listing_page_url".to_string(),
            ));
        }
        if source.query_url.trim().is_empty() {
            return Err(IdxError::ParseError(
                "ownership snapshot manifest is missing source.query_url".to_string(),
            ));
        }
        if source.pdf_url.trim().is_empty() {
            return Err(IdxError::ParseError(
                "ownership snapshot manifest is missing source.pdf_url".to_string(),
            ));
        }
        if source.title.trim().is_empty() {
            return Err(IdxError::ParseError(
                "ownership snapshot manifest is missing source.title".to_string(),
            ));
        }
        if source.publish_date.trim().is_empty() {
            return Err(IdxError::ParseError(
                "ownership snapshot manifest is missing source.publish_date".to_string(),
            ));
        }
    }

    let snapshot = &manifest.snapshot;
    if snapshot.kind.trim() != "sqlite" {
        return Err(IdxError::Unsupported(format!(
            "ownership snapshot kind `{}` is not supported; expected `sqlite`",
            snapshot.kind
        )));
    }
    if snapshot.compression.trim() != "none" {
        return Err(IdxError::Unsupported(format!(
            "ownership snapshot compression `{}` is not supported; expected `none`",
            snapshot.compression
        )));
    }
    if snapshot.version.trim().is_empty() {
        return Err(IdxError::ParseError(
            "ownership snapshot manifest is missing snapshot.version".to_string(),
        ));
    }
    if snapshot.download_url.trim().is_empty() {
        return Err(IdxError::ParseError(
            "ownership snapshot manifest is missing snapshot.download_url".to_string(),
        ));
    }
    if snapshot.size_bytes == 0 {
        return Err(IdxError::ParseError(
            "ownership snapshot manifest reported size_bytes=0".to_string(),
        ));
    }
    if snapshot.release_count == 0 {
        return Err(IdxError::ParseError(
            "ownership snapshot manifest reported release_count=0".to_string(),
        ));
    }
    if snapshot.latest_row_count == 0 {
        return Err(IdxError::ParseError(
            "ownership snapshot manifest reported latest_row_count=0".to_string(),
        ));
    }
    if snapshot.ticker_count == 0 {
        return Err(IdxError::ParseError(
            "ownership snapshot manifest reported ticker_count=0".to_string(),
        ));
    }

    validate_sha256_hex(&snapshot.sqlite_sha256, "snapshot.sqlite_sha256")?;
    validate_sha256_hex(
        &snapshot.latest_release_sha256,
        "snapshot.latest_release_sha256",
    )?;

    Ok(())
}

fn validate_sha256_hex(value: &str, field_name: &str) -> Result<(), IdxError> {
    let trimmed = value.trim();
    if trimmed.len() != 64 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(IdxError::ParseError(format!(
            "ownership snapshot manifest field `{field_name}` must be a 64-character hex sha256"
        )));
    }
    Ok(())
}

fn inspect_local_db(db_path: &Path) -> Result<Option<LocalSnapshotState>, IdxError> {
    if !db_path.exists() {
        return Ok(None);
    }

    let conn = Connection::open(db_path).map_err(|e| {
        IdxError::DatabaseError(format!("failed to open {}: {e}", db_path.display()))
    })?;
    db::ensure_schema(&conn)?;
    let releases = db::query_releases(&conn)?;

    Ok(Some(LocalSnapshotState {
        latest_release: releases.first().cloned(),
        release_count: releases.len(),
    }))
}

fn build_sync_decision(
    local: Option<&LocalSnapshotState>,
    snapshot: &OwnershipSnapshotArtifact,
    force: bool,
) -> SyncDecision {
    if force {
        return SyncDecision {
            action: if local.is_some() {
                OwnershipSyncAction::Refreshed
            } else {
                OwnershipSyncAction::Installed
            },
            should_download: true,
            reason: "force refresh requested".to_string(),
        };
    }

    let Some(local) = local else {
        return SyncDecision {
            action: OwnershipSyncAction::Installed,
            should_download: true,
            reason: "local ownership database does not exist yet".to_string(),
        };
    };

    let Some(latest) = &local.latest_release else {
        return SyncDecision {
            action: OwnershipSyncAction::Updated,
            should_download: true,
            reason: "local ownership database exists but has no imported releases".to_string(),
        };
    };

    if latest.as_of_date > snapshot.latest_as_of_date {
        return SyncDecision {
            action: OwnershipSyncAction::SkippedNewer,
            should_download: false,
            reason: format!(
                "local ownership database is newer than the published snapshot (local {} > snapshot {})",
                latest.as_of_date.format("%Y-%m-%d"),
                snapshot.latest_as_of_date.format("%Y-%m-%d")
            ),
        };
    }

    if latest.as_of_date < snapshot.latest_as_of_date {
        return SyncDecision {
            action: OwnershipSyncAction::Updated,
            should_download: true,
            reason: format!(
                "published snapshot advances local ownership data from {} to {}",
                latest.as_of_date.format("%Y-%m-%d"),
                snapshot.latest_as_of_date.format("%Y-%m-%d")
            ),
        };
    }

    if latest.sha256 == snapshot.latest_release_sha256 {
        if latest.row_count != snapshot.latest_row_count {
            return SyncDecision {
                action: OwnershipSyncAction::Updated,
                should_download: true,
                reason: "local ownership database has the same latest release date but different row_count metadata".to_string(),
            };
        }

        if local.release_count < snapshot.release_count {
            return SyncDecision {
                action: OwnershipSyncAction::Updated,
                should_download: true,
                reason: format!(
                    "local ownership database is missing historical releases (local {} < snapshot {})",
                    local.release_count, snapshot.release_count
                ),
            };
        }

        if local.release_count == snapshot.release_count {
            return SyncDecision {
                action: OwnershipSyncAction::NoChange,
                should_download: false,
                reason: format!(
                    "ownership snapshot already current at {} with {} release(s)",
                    snapshot.latest_as_of_date.format("%Y-%m-%d"),
                    snapshot.release_count
                ),
            };
        }
    }

    SyncDecision {
        action: OwnershipSyncAction::SkippedDiverged,
        should_download: false,
        reason: format!(
            "local ownership database differs from the published snapshot for {}; use --force to replace it",
            snapshot.latest_as_of_date.format("%Y-%m-%d")
        ),
    }
}

fn validate_downloaded_bytes(
    bytes: &[u8],
    snapshot: &OwnershipSnapshotArtifact,
) -> Result<(), IdxError> {
    let size_bytes = u64::try_from(bytes.len()).map_err(|e| {
        IdxError::ParseError(format!("snapshot download too large to validate: {e}"))
    })?;
    if size_bytes != snapshot.size_bytes {
        return Err(IdxError::ParseError(format!(
            "ownership snapshot size mismatch: manifest expected {} bytes, downloaded {} bytes",
            snapshot.size_bytes, size_bytes
        )));
    }

    let actual_sha256 = sha256_hex(bytes);
    if actual_sha256 != snapshot.sqlite_sha256 {
        return Err(IdxError::ParseError(format!(
            "ownership snapshot checksum mismatch: manifest expected {}, downloaded {}",
            snapshot.sqlite_sha256, actual_sha256
        )));
    }

    Ok(())
}

fn validate_snapshot_db(path: &Path, snapshot: &OwnershipSnapshotArtifact) -> Result<(), IdxError> {
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| {
            IdxError::DatabaseError(format!(
                "failed to open downloaded snapshot {}: {e}",
                path.display()
            ))
        })?;

    let quick_check: String = conn
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|e| IdxError::DatabaseError(format!("snapshot quick_check failed: {e}")))?;
    if quick_check.trim() != "ok" {
        return Err(IdxError::DatabaseError(format!(
            "snapshot quick_check failed: {quick_check}"
        )));
    }

    let releases = db::query_releases(&conn)?;
    if releases.len() != snapshot.release_count {
        return Err(IdxError::ParseError(format!(
            "downloaded snapshot release_count mismatch: manifest expected {}, sqlite has {}",
            snapshot.release_count,
            releases.len()
        )));
    }

    let latest = releases.first().ok_or_else(|| {
        IdxError::ParseError("downloaded snapshot sqlite had no ownership releases".to_string())
    })?;

    if latest.as_of_date != snapshot.latest_as_of_date {
        return Err(IdxError::ParseError(format!(
            "downloaded snapshot latest_as_of_date mismatch: manifest expected {}, sqlite has {}",
            snapshot.latest_as_of_date.format("%Y-%m-%d"),
            latest.as_of_date.format("%Y-%m-%d")
        )));
    }
    if latest.sha256 != snapshot.latest_release_sha256 {
        return Err(IdxError::ParseError(format!(
            "downloaded snapshot latest_release_sha256 mismatch: manifest expected {}, sqlite has {}",
            snapshot.latest_release_sha256, latest.sha256
        )));
    }
    if latest.row_count != snapshot.latest_row_count {
        return Err(IdxError::ParseError(format!(
            "downloaded snapshot latest_row_count mismatch: manifest expected {}, sqlite has {}",
            snapshot.latest_row_count, latest.row_count
        )));
    }

    let ticker_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tickers", [], |row| row.get(0))
        .map_err(|e| IdxError::DatabaseError(format!("failed counting snapshot tickers: {e}")))?;
    let ticker_count = usize::try_from(ticker_count)
        .map_err(|e| IdxError::DatabaseError(format!("invalid snapshot ticker_count: {e}")))?;
    if ticker_count != snapshot.ticker_count {
        return Err(IdxError::ParseError(format!(
            "downloaded snapshot ticker_count mismatch: manifest expected {}, sqlite has {}",
            snapshot.ticker_count, ticker_count
        )));
    }

    Ok(())
}

fn build_temp_path(db_path: &Path) -> PathBuf {
    let file_name = db_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ownership.db");
    let temp_name = format!(".{file_name}.sync-{}.tmp", fastrand::u64(..));
    match db_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(temp_name),
        _ => PathBuf::from(temp_name),
    }
}

fn install_snapshot_file(temp_path: &Path, db_path: &Path) -> Result<(), IdxError> {
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| {
            IdxError::Io(format!(
                "failed to create ownership database directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    let backup_path = build_backup_path(db_path);
    if backup_path.exists() {
        fs::remove_file(&backup_path).map_err(|e| {
            IdxError::Io(format!(
                "failed to remove stale backup {}: {e}",
                backup_path.display()
            ))
        })?;
    }

    let had_existing = db_path.exists();
    remove_sqlite_sidecars(db_path)?;

    if had_existing {
        fs::rename(db_path, &backup_path).map_err(|e| {
            IdxError::Io(format!(
                "failed to move existing ownership database {} to backup {}: {e}",
                db_path.display(),
                backup_path.display()
            ))
        })?;
    }

    if let Err(err) = fs::rename(temp_path, db_path) {
        if had_existing {
            let _ = fs::rename(&backup_path, db_path);
        }
        return Err(IdxError::Io(format!(
            "failed to install ownership snapshot into {}: {err}",
            db_path.display()
        )));
    }

    if had_existing && backup_path.exists() {
        fs::remove_file(&backup_path).map_err(|e| {
            IdxError::Io(format!(
                "failed to remove snapshot backup {}: {e}",
                backup_path.display()
            ))
        })?;
    }

    remove_sqlite_sidecars(db_path)?;
    Ok(())
}

fn build_backup_path(db_path: &Path) -> PathBuf {
    let file_name = db_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ownership.db");
    let backup_name = format!(".{file_name}.sync-backup");
    match db_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(backup_name),
        _ => PathBuf::from(backup_name),
    }
}

fn remove_sqlite_sidecars(db_path: &Path) -> Result<(), IdxError> {
    for suffix in ["-wal", "-shm"] {
        let file_name = format!(
            "{}{suffix}",
            db_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("ownership.db")
        );
        let sidecar = match db_path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.join(&file_name),
            _ => PathBuf::from(&file_name),
        };

        if sidecar.exists() {
            fs::remove_file(&sidecar).map_err(|e| {
                IdxError::Io(format!(
                    "failed to remove SQLite sidecar {}: {e}",
                    sidecar.display()
                ))
            })?;
        }
    }

    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_text(source: &str, context: &str) -> Result<String, IdxError> {
    if is_http_source(source) {
        let response = ureq::get(source)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json,text/plain;q=0.9,*/*;q=0.8")
            .call()
            .map_err(|e| IdxError::Http(format!("failed to fetch {context}: {e}")))?;
        let mut body = response.into_body();
        return body
            .read_to_string()
            .map_err(|e| IdxError::Http(format!("failed reading {context} body: {e}")));
    }

    let path = local_source_path(source);
    fs::read_to_string(&path)
        .map_err(|e| IdxError::Io(format!("failed to read {context} {}: {e}", path.display())))
}

fn read_bytes(source: &str, context: &str) -> Result<Vec<u8>, IdxError> {
    if is_http_source(source) {
        let response = ureq::get(source)
            .header("User-Agent", USER_AGENT)
            .header(
                "Accept",
                "application/octet-stream,application/x-sqlite3,*/*;q=0.8",
            )
            .call()
            .map_err(|e| IdxError::Http(format!("failed to fetch {context}: {e}")))?;
        let mut body = response.into_body();
        return body
            .read_to_vec()
            .map_err(|e| IdxError::Http(format!("failed reading {context} body: {e}")));
    }

    let path = local_source_path(source);
    fs::read(&path)
        .map_err(|e| IdxError::Io(format!("failed to read {context} {}: {e}", path.display())))
}

fn is_http_source(source: &str) -> bool {
    let normalized = source.trim().to_ascii_lowercase();
    normalized.starts_with("http://") || normalized.starts_with("https://")
}

fn local_source_path(source: &str) -> PathBuf {
    if let Some(path) = source.trim().strip_prefix("file://") {
        return PathBuf::from(path);
    }
    PathBuf::from(source.trim())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rusqlite::Connection;

    use super::{
        DEFAULT_SNAPSHOT_MANIFEST_URL, OwnershipSnapshotArtifact, OwnershipSnapshotManifest,
        OwnershipSnapshotSource, OwnershipSyncAction, SNAPSHOT_MANIFEST_SCHEMA_VERSION,
        build_sync_decision, parse_manifest, resolve_manifest_source_with,
    };
    use crate::ownership::db::{ensure_schema, insert_release};
    use crate::ownership::types::OwnershipRelease;

    #[test]
    fn parse_manifest_rejects_unknown_schema_version() {
        let raw = r#"{
          "schema_version": 99,
          "generated_at": "2026-03-31T12:00:00Z",
          "snapshot": {
            "kind": "sqlite",
            "compression": "none",
            "version": "2026-02-27",
            "download_url": "/tmp/ownership.sqlite",
            "sqlite_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "size_bytes": 123,
            "release_count": 2,
            "latest_as_of_date": "2026-02-27",
            "latest_release_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "latest_row_count": 100,
            "ticker_count": 5
          }
        }"#;

        let err = parse_manifest(raw).expect_err("schema_version should fail");
        assert!(
            err.to_string()
                .contains("unsupported ownership snapshot manifest schema_version")
        );
    }

    #[test]
    fn build_sync_decision_updates_same_latest_release_when_history_is_incomplete() {
        let snapshot = OwnershipSnapshotArtifact {
            kind: "sqlite".to_string(),
            compression: "none".to_string(),
            version: "2026-02-27".to_string(),
            download_url: "/tmp/ownership.sqlite".to_string(),
            sqlite_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            size_bytes: 123,
            release_count: 2,
            latest_as_of_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
            latest_release_sha256:
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            latest_row_count: 7261,
            ticker_count: 955,
        };
        let local = super::LocalSnapshotState {
            latest_release: Some(OwnershipRelease {
                id: 1,
                source_url: None,
                sha256: snapshot.latest_release_sha256.clone(),
                as_of_date: snapshot.latest_as_of_date,
                row_count: snapshot.latest_row_count,
                imported_at: 0,
            }),
            release_count: 1,
        };

        let decision = build_sync_decision(Some(&local), &snapshot, false);
        assert_eq!(decision.action, OwnershipSyncAction::Updated);
        assert!(decision.should_download);
        assert!(decision.reason.contains("missing historical releases"));
    }

    #[test]
    fn manifest_round_trip_parses_valid_payload() {
        let manifest = OwnershipSnapshotManifest {
            schema_version: SNAPSHOT_MANIFEST_SCHEMA_VERSION,
            generated_at: "2026-03-31T12:00:00Z".to_string(),
            source: Some(OwnershipSnapshotSource {
                family: "above1".to_string(),
                listing_page_url: "https://www.idx.co.id/id/berita/pengumuman/".to_string(),
                query_url: "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement?keywords=pemegang%20saham%20di%20atas%201&pageNumber=1&pageSize=10&lang=id".to_string(),
                pdf_url: "https://www.idx.co.id/StaticData/sample.pdf".to_string(),
                title: "Pemegang Saham di atas 1% (KSEI)".to_string(),
                publish_date: "2026-03-10T00:00:00".to_string(),
                original_filename: Some("sample.pdf".to_string()),
            }),
            snapshot: OwnershipSnapshotArtifact {
                kind: "sqlite".to_string(),
                compression: "none".to_string(),
                version: "2026-02-27".to_string(),
                download_url: "/tmp/ownership.sqlite".to_string(),
                sqlite_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
                size_bytes: 123,
                release_count: 1,
                latest_as_of_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
                latest_release_sha256:
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
                latest_row_count: 7261,
                ticker_count: 955,
            },
        };

        let raw = serde_json::to_string(&manifest).unwrap();
        let parsed = parse_manifest(&raw).expect("valid manifest");
        assert_eq!(parsed, manifest);
    }

    #[test]
    fn manifest_without_source_metadata_still_parses() {
        let raw = r#"{
          "schema_version": 1,
          "generated_at": "2026-03-31T12:00:00Z",
          "snapshot": {
            "kind": "sqlite",
            "compression": "none",
            "version": "2026-02-27",
            "download_url": "/tmp/ownership.sqlite",
            "sqlite_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "size_bytes": 123,
            "release_count": 1,
            "latest_as_of_date": "2026-02-27",
            "latest_release_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "latest_row_count": 7261,
            "ticker_count": 955
          }
        }"#;

        let parsed = parse_manifest(raw).expect("old manifest shape should still parse");
        assert!(parsed.source.is_none());
    }

    #[test]
    fn resolve_manifest_source_uses_built_in_default_when_unset() {
        let resolved = resolve_manifest_source_with(None, None, None).expect("default manifest");
        assert_eq!(resolved, DEFAULT_SNAPSHOT_MANIFEST_URL);
    }

    #[test]
    fn resolve_manifest_source_prefers_explicit_then_env_then_config() {
        let explicit = resolve_manifest_source_with(
            Some("https://example.com/explicit.json"),
            Some("https://example.com/env.json"),
            Some("https://example.com/config.json"),
        )
        .expect("explicit manifest");
        assert_eq!(explicit, "https://example.com/explicit.json");

        let env = resolve_manifest_source_with(
            Some("   "),
            Some("https://example.com/env.json"),
            Some("https://example.com/config.json"),
        )
        .expect("env manifest");
        assert_eq!(env, "https://example.com/env.json");

        let config =
            resolve_manifest_source_with(None, Some(" "), Some("https://example.com/config.json"))
                .expect("config manifest");
        assert_eq!(config, "https://example.com/config.json");
    }

    #[test]
    fn ensure_schema_can_store_release_metadata_needed_for_snapshots() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        insert_release(
            &conn,
            &OwnershipRelease {
                id: 0,
                source_url: Some("https://example.com/ownership.sqlite".to_string()),
                sha256: "abc".to_string(),
                as_of_date: NaiveDate::from_ymd_opt(2026, 2, 27).unwrap(),
                row_count: 1,
                imported_at: 1,
            },
        )
        .unwrap();

        let release_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ownership_releases", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(release_count, 1);
    }
}
