use clap::{Args, Subcommand};

use crate::cache::Cache;
use crate::error::IdxError;
use crate::runtime;

#[derive(Debug, Args)]
#[command(about = "Manage local cache")]
pub struct CacheCmd {
    #[command(subcommand)]
    pub command: CacheSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CacheSubcommand {
    #[command(about = "Show cache usage information")]
    Info,
    #[command(about = "Clear all cached files")]
    Clear,
}

pub fn handle(cmd: &CacheCmd) -> Result<(), IdxError> {
    let cache = Cache::new()?;
    match &cmd.command {
        CacheSubcommand::Info => {
            let info = cache.info()?;
            println!("path: {}", info.path.display());
            println!("files: {}", info.files);
            println!("size_bytes: {}", info.total_size);
            println!(
                "oldest: {}",
                info.oldest
                    .map(|v| v.to_rfc3339())
                    .unwrap_or_else(|| "-".to_string())
            );
            println!(
                "newest: {}",
                info.newest
                    .map(|v| v.to_rfc3339())
                    .unwrap_or_else(|| "-".to_string())
            );
        }
        CacheSubcommand::Clear => {
            let (removed, failed) = cache.clear()?;
            println!("cleared {removed} files");
            if !failed.is_empty() {
                runtime::warn(format!("failed to remove {} file(s)", failed.len()));
            }
        }
    }
    Ok(())
}
