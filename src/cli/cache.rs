use clap::{Args, Subcommand};

use crate::cache::Cache;
use crate::error::IdxError;

#[derive(Debug, Args)]
pub struct CacheCmd {
    #[command(subcommand)]
    pub command: CacheSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CacheSubcommand {
    Info,
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
            let removed = cache.clear()?;
            println!("cleared {removed} files");
        }
    }
    Ok(())
}
