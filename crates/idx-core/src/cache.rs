use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::IdxError;
use crate::runtime;

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

/// A cache hit with its metadata.
#[derive(Debug)]
pub struct Cached<T> {
    pub data: T,
    pub fetched_at: DateTime<Utc>,
    pub expired: bool,
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

    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn get<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<T>, IdxError> {
        Ok(self.fresh(data_type, symbol)?.map(|hit| hit.data))
    }

    pub fn get_stale<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<T>, IdxError> {
        Ok(self.any(data_type, symbol)?.map(|hit| hit.data))
    }

    pub fn get_offline<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<T>, IdxError> {
        Ok(self.offline(data_type, symbol)?.map(|hit| hit.data))
    }

    /// An unexpired entry.
    pub fn fresh<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<Cached<T>>, IdxError> {
        Ok(self.any(data_type, symbol)?.filter(|hit| !hit.expired))
    }

    /// Any entry, expired or not.
    pub fn any<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<Cached<T>>, IdxError> {
        Ok(self
            .read_entry::<T>(data_type, symbol)?
            .map(|entry| Cached {
                expired: is_expired(entry.fetched_at, entry.ttl_secs),
                fetched_at: entry.fetched_at,
                data: entry.data,
            }))
    }

    /// Any entry, warning on stderr when serving an expired one offline.
    pub fn offline<T: DeserializeOwned>(
        &self,
        data_type: &str,
        symbol: &str,
    ) -> Result<Option<Cached<T>>, IdxError> {
        let hit = self.any::<T>(data_type, symbol)?;
        if let Some(hit) = &hit
            && hit.expired
        {
            runtime::warn(format!(
                "offline: serving expired cache for {symbol} (fetched {})",
                hit.fetched_at.to_rfc3339()
            ));
        }
        Ok(hit)
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
        // Write then rename, so a concurrent reader never sees a partial
        // entry (and deletes it as corrupt).
        let temp = path.with_extension(format!("json.{}.tmp", fastrand::u64(..)));
        fs::write(&temp, raw).map_err(|e| IdxError::Io(e.to_string()))?;
        fs::rename(&temp, &path).map_err(|e| {
            let _ = fs::remove_file(&temp);
            IdxError::Io(e.to_string())
        })
    }

    pub fn info(&self) -> Result<CacheInfo, IdxError> {
        let mut files = 0usize;
        let mut total_size = 0u64;
        let mut oldest: Option<DateTime<Utc>> = None;
        let mut newest: Option<DateTime<Utc>> = None;

        if self.root.exists() {
            Self::walk(&self.root, &mut |p| {
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
        // `walk` yields files and symlinks, never entering a linked
        // directory; remove_file on a symlink removes the link, not its target.
        Self::walk(&self.root, &mut |p| match fs::remove_file(p) {
            Ok(_) => removed += 1,
            Err(_) => failed.push(p.to_path_buf()),
        })?;
        Ok((removed, failed))
    }

    fn walk<F: FnMut(&Path)>(dir: &Path, f: &mut F) -> Result<(), IdxError> {
        for entry in fs::read_dir(dir).map_err(|e| IdxError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| IdxError::Io(e.to_string()))?;
            let path = entry.path();
            // file_type() does not follow symlinks, so a link to a directory
            // outside the cache is never walked (or emptied by `clear`).
            let file_type = entry.file_type().map_err(|e| IdxError::Io(e.to_string()))?;
            if file_type.is_dir() {
                Self::walk(&path, f)?;
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
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                return Err(IdxError::Io(e.to_string()));
            }
        };
        let entry: CacheEntry<T> = match serde_json::from_str(&raw) {
            Ok(e) => e,
            Err(e) => {
                runtime::warn(format!(
                    "corrupted cache entry for {data_type}/{symbol}, treating as miss: {e}"
                ));
                // Not deleted: a concurrent writer may have just replaced it
                // with a valid entry. The next put overwrites it atomically.
                return Ok(None);
            }
        };
        if entry.schema_version != CURRENT_SCHEMA_VERSION {
            runtime::info(format!(
                "cache schema mismatch for {} (got {}, expected {}), treating as miss",
                path.display(),
                entry.schema_version,
                CURRENT_SCHEMA_VERSION
            ));
            return Ok(None);
        }
        Ok(Some(entry))
    }

    fn entry_path(&self, data_type: &str, symbol: &str) -> PathBuf {
        self.root
            .join(safe_file_name(data_type))
            .join(format!("{}.json", safe_file_name(symbol)))
    }
}

/// Keys become single path components: anything that could separate or
/// climb directories is percent-encoded. Keys made of letters, digits, and
/// `._-:^=` (every key idx produces) are unchanged.
fn safe_file_name(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for (i, byte) in key.bytes().enumerate() {
        let safe = byte.is_ascii_alphanumeric()
            || matches!(byte, b'_' | b'-' | b':' | b'^' | b'=')
            || (byte == b'.' && i > 0);
        if safe {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn is_expired(fetched_at: DateTime<Utc>, ttl_secs: u64) -> bool {
    let ttl =
        chrono::Duration::from_std(Duration::from_secs(ttl_secs)).unwrap_or(chrono::Duration::MAX);
    Utc::now().signed_duration_since(fetched_at) >= ttl
}

pub fn cache_dir() -> Result<PathBuf, IdxError> {
    if let Ok(dir) = std::env::var("XDG_CACHE_HOME")
        && !dir.is_empty()
    {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return Ok(path.join("idx"));
        }
    }
    ProjectDirs::from("", "", "idx")
        .map(|d| d.cache_dir().to_path_buf())
        .ok_or_else(|| IdxError::ConfigError("unable to resolve cache dir".to_string()))
}

#[cfg(test)]
mod tests {

    #[cfg(unix)]
    #[test]
    fn clear_removes_symlinks_without_touching_their_targets() {
        let base = std::env::temp_dir().join(format!("idx-cache-link-{}", fastrand::u64(..)));
        let outside = base.join("documents");
        let root = base.join("cache");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(root.join("quote")).unwrap();
        std::fs::write(outside.join("keep.txt"), b"mine").unwrap();
        std::fs::write(root.join("quote").join("BBCA.json"), b"{}").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        let (removed, failed) = super::Cache::at(root.clone()).clear().unwrap();

        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(removed, 2, "the cache file and the link itself");
        assert!(
            outside.join("keep.txt").exists(),
            "the link's target is left alone"
        );
        assert!(!root.join("link").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde::{Deserialize, Serialize};

    use super::{Cache, CacheEntry, safe_file_name};

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
        let cache = Cache::at(root.clone());

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
    fn corrupted_cache_entry_is_a_miss_until_replaced() {
        let root = tmp();
        let cache = Cache::at(root.clone());

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

        // Left for the next put to replace (deleting could race a writer).
        cache
            .put("quote", "CORRUPT.JK", &T { v: 7 }, 3600)
            .expect("put");
        let result: Option<T> = cache.get("quote", "CORRUPT.JK").expect("get");
        assert_eq!(result, Some(T { v: 7 }));
    }

    #[test]
    fn corrupted_cache_entry_get_stale_is_a_miss() {
        let root = tmp();
        let cache = Cache::at(root.clone());

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

        assert!(path.exists(), "left for the next put to replace");
    }

    #[test]
    fn cache_keys_cannot_leave_the_cache_root() {
        assert_eq!(safe_file_name("BBCA.JK-3mo-1d"), "BBCA.JK-3mo-1d");
        assert_eq!(
            safe_file_name("top-performers:id:50"),
            "top-performers:id:50"
        );
        assert_eq!(safe_file_name("../../REPORT"), "%2E.%2F..%2FREPORT");
        assert_eq!(safe_file_name(".."), "%2E.");

        let cache = Cache::at(std::path::PathBuf::from("/cache/idx"));
        let path = cache.entry_path("msn-quote", "../../REPORT");
        assert_eq!(
            path.parent().unwrap(),
            std::path::Path::new("/cache/idx/msn-quote")
        );
    }
}
