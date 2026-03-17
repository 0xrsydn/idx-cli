use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::IdxError;

const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry<T> {
    fetched_at: DateTime<Utc>,
    ttl_secs: u64,
    schema_version: u32,
    data: T,
}

#[derive(Debug)]
pub struct CacheInfo {
    pub path: PathBuf,
    pub files: usize,
    pub total_size: u64,
    pub oldest: Option<DateTime<Utc>>,
    pub newest: Option<DateTime<Utc>>,
}

impl Cache {
    pub fn new() -> Result<Self, IdxError> {
        Ok(Self { root: cache_dir()? })
    }

    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn get<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<T>, IdxError> {
        let Some(entry): Option<CacheEntry<T>> = self.read_entry(data_type, symbol)? else {
            return Ok(None);
        };
        let age = Utc::now().signed_duration_since(entry.fetched_at);
        if age
            >= chrono::Duration::from_std(Duration::from_secs(entry.ttl_secs))
                .map_err(|e| IdxError::CacheMiss(e.to_string()))?
        {
            return Ok(None);
        }
        Ok(Some(entry.data))
    }

    pub fn get_stale<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<T>, IdxError> {
        Ok(self.read_entry::<T>(data_type, symbol)?.map(|e| e.data))
    }

    pub fn put<T: Serialize>(
        &self,
        data_type: &str,
        symbol: &str,
        data: &T,
        ttl_secs: u64,
    ) -> Result<(), IdxError> {
        let path = self.entry_path(data_type, symbol);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| IdxError::Io(e.to_string()))?;
        }
        let entry = CacheEntry {
            fetched_at: Utc::now(),
            ttl_secs,
            schema_version: CURRENT_SCHEMA_VERSION,
            data,
        };
        let raw = serde_json::to_string_pretty(&entry)
            .map_err(|e| IdxError::ParseError(e.to_string()))?;
        fs::write(path, raw).map_err(|e| IdxError::Io(e.to_string()))
    }

    pub fn info(&self) -> Result<CacheInfo, IdxError> {
        let mut files = 0usize;
        let mut total_size = 0u64;
        let mut oldest: Option<DateTime<Utc>> = None;
        let mut newest: Option<DateTime<Utc>> = None;

        if self.root.exists() {
            self.walk(&self.root, &mut |p| {
                if let Ok(meta) = fs::metadata(p)
                    && meta.is_file()
                {
                    files += 1;
                    total_size += meta.len();
                    if let Ok(raw) = fs::read_to_string(p)
                        && let Ok(entry) =
                            serde_json::from_str::<CacheEntry<serde_json::Value>>(&raw)
                    {
                        oldest = Some(oldest.map_or(entry.fetched_at, |o| o.min(entry.fetched_at)));
                        newest = Some(newest.map_or(entry.fetched_at, |n| n.max(entry.fetched_at)));
                    }
                }
            })?;
        }

        Ok(CacheInfo {
            path: self.root.clone(),
            files,
            total_size,
            oldest,
            newest,
        })
    }

    pub fn clear(&self) -> Result<(usize, Vec<PathBuf>), IdxError> {
        if !self.root.exists() {
            return Ok((0, Vec::new()));
        }
        let mut removed = 0usize;
        let mut failed = Vec::new();
        self.walk(&self.root, &mut |p| {
            if p.is_file() {
                match fs::remove_file(p) {
                    Ok(_) => removed += 1,
                    Err(_) => failed.push(p.to_path_buf()),
                }
            }
        })?;
        Ok((removed, failed))
    }

    fn walk<F: FnMut(&Path)>(&self, dir: &Path, f: &mut F) -> Result<(), IdxError> {
        for entry in fs::read_dir(dir).map_err(|e| IdxError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| IdxError::Io(e.to_string()))?;
            let path = entry.path();
            if path.is_dir() {
                self.walk(&path, f)?;
            } else {
                f(&path);
            }
        }
        Ok(())
    }

    fn read_entry<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<CacheEntry<T>>, IdxError> {
        let path = self.entry_path(data_type, symbol);
        if !path.exists() {
            return Ok(None);
        }
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "warning: corrupted cache entry for {}/{}, treating as miss: {}",
                    data_type, symbol, e
                );
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
        };
        let entry: CacheEntry<T> = match serde_json::from_str(&raw) {
            Ok(e) => e,
            Err(e) => {
                eprintln!(
                    "warning: corrupted cache entry for {}/{}, treating as miss: {}",
                    data_type, symbol, e
                );
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
        };
        if entry.schema_version != CURRENT_SCHEMA_VERSION {
            eprintln!(
                "debug: cache schema mismatch for {} (got {}, expected {})",
                path.display(),
                entry.schema_version,
                CURRENT_SCHEMA_VERSION
            );
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
        Ok(Some(entry))
    }

    fn entry_path(&self, data_type: &str, symbol: &str) -> PathBuf {
        self.root.join(data_type).join(format!("{symbol}.json"))
    }
}

pub fn cache_dir() -> Result<PathBuf, IdxError> {
    if let Ok(dir) = std::env::var("XDG_CACHE_HOME")
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir).join("idx"));
    }
    ProjectDirs::from("", "", "idx")
        .map(|d| d.cache_dir().to_path_buf())
        .ok_or_else(|| IdxError::ConfigError("unable to resolve cache dir".to_string()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde::{Deserialize, Serialize};

    use super::{Cache, CacheEntry};

    static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct T {
        v: i32,
    }

    fn tmp() -> std::path::PathBuf {
        let suffix = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("idx-cache-test-{}-{suffix}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).expect("create tmp cache dir");
        p
    }

    #[test]
    fn write_read_expire_and_stale() {
        let root = tmp();
        let cache = Cache::with_root(root.clone());

        cache
            .put("quote", "BBCA.JK", &T { v: 7 }, 300)
            .expect("cache write");
        let fresh: Option<T> = cache.get("quote", "BBCA.JK").expect("cache read fresh");
        assert_eq!(fresh, Some(T { v: 7 }));

        let path = root.join("quote/BBCA.JK.json");
        let mut entry: CacheEntry<T> =
            serde_json::from_str(&fs::read_to_string(&path).expect("read cache file"))
                .expect("parse cache entry");
        entry.fetched_at = chrono::Utc::now() - chrono::Duration::seconds(1000);
        fs::write(
            &path,
            serde_json::to_string(&entry).expect("serialize entry"),
        )
        .expect("write old entry");

        let expired: Option<T> = cache.get("quote", "BBCA.JK").expect("cache read expired");
        assert_eq!(expired, None);

        let stale: Option<T> = cache
            .get_stale("quote", "BBCA.JK")
            .expect("cache read stale");
        assert_eq!(stale, Some(T { v: 7 }));
    }

    #[test]
    fn corrupted_cache_entry_returns_none_and_deletes_file() {
        let root = tmp();
        let cache = Cache::with_root(root.clone());

        // Write invalid JSON to a cache file
        let path = root.join("quote/CORRUPT.JK.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, "this is not valid json {{{").expect("write corrupted cache");

        assert!(path.exists(), "corrupted file should exist before get()");

        // get() should return Ok(None), not an error
        let result: Option<T> = cache
            .get("quote", "CORRUPT.JK")
            .expect("get should not error");
        assert_eq!(result, None, "corrupted entry should be treated as miss");

        // The corrupted file should be deleted
        assert!(!path.exists(), "corrupted file should be deleted");
    }

    #[test]
    fn corrupted_cache_entry_get_stale_returns_none_and_deletes_file() {
        let root = tmp();
        let cache = Cache::with_root(root.clone());

        // Write invalid JSON to a cache file
        let path = root.join("quote/STALE_CORRUPT.JK.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, "{ not valid json at all").expect("write corrupted cache");

        assert!(
            path.exists(),
            "corrupted file should exist before get_stale()"
        );

        // get_stale() should return Ok(None), not an error
        let result: Option<T> = cache
            .get_stale("quote", "STALE_CORRUPT.JK")
            .expect("get_stale should not error");
        assert_eq!(
            result, None,
            "corrupted entry should be treated as miss in get_stale"
        );

        // The corrupted file should be deleted
        assert!(!path.exists(), "corrupted file should be deleted");
    }
}
