pub mod archive;
pub mod db;
pub mod entities;
pub mod graph;
pub mod parser;
pub mod remote;
pub mod search;
pub mod snapshot;
pub mod types;
pub mod xlsx;

use std::io::Read;

use crate::error::IdxError;

/// Largest decompressed ZIP entry read into memory. Real IDX worksheets and
/// KSEI TXT archives are tens of MB; this bounds a hostile "zip bomb".
const MAX_ZIP_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// Reads one decompressed ZIP entry as UTF-8, failing past the size limit.
pub(crate) fn read_zip_entry_bounded(entry: impl Read, what: &str) -> Result<String, IdxError> {
    let mut bytes = Vec::new();
    entry
        .take(MAX_ZIP_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| IdxError::ParseError(format!("failed to read {what}: {e}")))?;
    if bytes.len() as u64 > MAX_ZIP_ENTRY_BYTES {
        return Err(IdxError::ParseError(format!(
            "{what} expands beyond {MAX_ZIP_ENTRY_BYTES} bytes"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|e| IdxError::ParseError(format!("failed to decode {what}: {e}")))
}
