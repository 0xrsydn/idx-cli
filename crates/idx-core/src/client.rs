//! The `Idx` entry point and the cache policy shared by live-data lookups.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::api::{SelectedProvider, default_provider};
use crate::cache::Cache;
use crate::config::CoreConfig;
use crate::error::IdxError;
use crate::runtime;

/// How a live-data lookup may use the on-disk cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CachePolicy {
    /// Serve unexpired cache, otherwise fetch and store; on a fetch failure,
    /// fall back to an expired entry.
    #[default]
    Default,
    /// Never touch the network; serve any cached entry, expired or not.
    Offline,
    /// Always fetch; neither read nor write the cache.
    Refresh,
}

impl CachePolicy {
    /// Maps the CLI's `--offline` / `--no-cache` flags, which are exclusive.
    pub fn from_flags(offline: bool, no_cache: bool) -> Result<Self, IdxError> {
        match (offline, no_cache) {
            (true, true) => Err(IdxError::InvalidInput(
                "cannot combine --offline with --no-cache".to_string(),
            )),
            (true, false) => Ok(Self::Offline),
            (false, true) => Ok(Self::Refresh),
            (false, false) => Ok(Self::Default),
        }
    }
}

/// Live data with where it came from and how fresh it is.
#[derive(Debug, Clone, Serialize)]
pub struct Fetched<T> {
    pub data: T,
    /// Provider that produced the data (`msn` or `yahoo`).
    pub source: &'static str,
    pub fetched_at: DateTime<Utc>,
    /// True when an expired cache entry was served (offline, or after a
    /// failed fetch).
    pub stale: bool,
}

impl<T> Fetched<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Fetched<U> {
        Fetched {
            data: f(self.data),
            source: self.source,
            fetched_at: self.fetched_at,
            stale: self.stale,
        }
    }
}

/// Entry point for market, financial, and analysis data.
pub struct Idx {
    pub(crate) config: CoreConfig,
    pub(crate) provider: SelectedProvider,
    pub(crate) cache: Cache,
}

impl Idx {
    pub fn new(config: CoreConfig) -> Result<Self, IdxError> {
        let cache = Cache::at(config.cache_dir()?);
        let provider = default_provider(config.provider, config.verbose);
        Ok(Self {
            config,
            provider,
            cache,
        })
    }

    pub fn config(&self) -> &CoreConfig {
        &self.config
    }

    pub fn market(&self) -> crate::market::Market<'_> {
        crate::market::Market { idx: self }
    }

    pub fn broker(&self) -> crate::broker::Broker<'_> {
        crate::broker::Broker { idx: self }
    }

    pub fn financial(&self) -> crate::financial::Financial<'_> {
        crate::financial::Financial { idx: self }
    }

    pub fn analysis(&self) -> crate::analysis::Analysis<'_> {
        crate::analysis::Analysis { idx: self }
    }

    #[cfg(feature = "ownership")]
    pub fn investor(&self) -> crate::investor::Investor {
        crate::investor::Investor::new(&self.config)
    }

    pub(crate) fn resolve(&self, symbol: &str) -> Result<String, IdxError> {
        crate::api::resolve_symbol(symbol, &self.config.exchange)
    }

    pub(crate) fn bucket(&self, key: &str) -> String {
        format!("{}-{key}", self.provider.kind().as_str())
    }
}

pub(crate) struct CacheSpec<'a> {
    pub bucket: &'a str,
    pub key: &'a str,
    pub ttl_secs: u64,
    /// Named in the stale-fallback warning.
    pub subject: &'a str,
    pub source: &'static str,
}

/// Fetches through the cache. `finish` turns the raw fetch into the cached
/// value; its errors propagate without a stale-cache fallback.
pub(crate) fn cached_fetch<R, T>(
    cache: &Cache,
    spec: CacheSpec<'_>,
    policy: CachePolicy,
    fetch: impl FnOnce() -> Result<R, IdxError>,
    finish: impl FnOnce(R) -> Result<T, IdxError>,
) -> Result<Fetched<T>, IdxError>
where
    T: Serialize + DeserializeOwned,
{
    let hit = |hit: crate::cache::Cached<T>, stale: bool| Fetched {
        data: hit.data,
        source: spec.source,
        fetched_at: hit.fetched_at,
        stale,
    };

    match policy {
        CachePolicy::Default => {
            if let Some(fresh) = cache.fresh::<T>(spec.bucket, spec.key)? {
                return Ok(hit(fresh, false));
            }
        }
        CachePolicy::Offline => {
            return cache
                .offline::<T>(spec.bucket, spec.key)?
                .map(|cached| {
                    let stale = cached.expired;
                    hit(cached, stale)
                })
                .ok_or_else(|| IdxError::CacheMiss(format!("{}/{}", spec.bucket, spec.key)));
        }
        CachePolicy::Refresh => {}
    }

    let use_cache = policy == CachePolicy::Default;
    match fetch() {
        Ok(raw) => {
            let data = finish(raw)?;
            if use_cache {
                cache.put(spec.bucket, spec.key, &data, spec.ttl_secs)?;
            }
            Ok(Fetched {
                data,
                source: spec.source,
                fetched_at: Utc::now(),
                stale: false,
            })
        }
        Err(err) => {
            if use_cache && let Some(stale) = cache.any::<T>(spec.bucket, spec.key)? {
                runtime::warn(format!(
                    "network failed, serving stale cache for {}",
                    spec.subject
                ));
                return Ok(hit(stale, true));
            }
            Err(err)
        }
    }
}
