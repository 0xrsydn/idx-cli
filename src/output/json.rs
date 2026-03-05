use serde::Serialize;

use crate::error::IdxError;

pub fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<(), IdxError> {
    let out =
        serde_json::to_string_pretty(value).map_err(|e| IdxError::ParseError(e.to_string()))?;
    println!("{out}");
    Ok(())
}
