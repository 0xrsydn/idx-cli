use serde::Serialize;
use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Error, Clone)]
pub enum IdxError {
    #[error("symbol not found: {0}")]
    SymbolNotFound(String),
    #[error("provider rate limited")]
    RateLimited,
    #[error("provider unavailable")]
    ProviderUnavailable,
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("cache miss: {0}")]
    CacheMiss(String),
    #[error("offline: {0}")]
    Offline(String),
    #[error("config error: {0}")]
    ConfigError(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("auth error: {0}")]
    AuthError(String),
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ErrorCode {
    SymbolNotFound,
    RateLimited,
    ProviderUnavailable,
    Unsupported,
    ParseError,
    CacheMiss,
    Offline,
    ConfigError,
    Io,
    Http,
    AuthError,
}

impl IdxError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::SymbolNotFound(_) => ErrorCode::SymbolNotFound,
            Self::RateLimited => ErrorCode::RateLimited,
            Self::ProviderUnavailable => ErrorCode::ProviderUnavailable,
            Self::Unsupported(_) => ErrorCode::Unsupported,
            Self::ParseError(_) => ErrorCode::ParseError,
            Self::CacheMiss(_) => ErrorCode::CacheMiss,
            Self::Offline(_) => ErrorCode::Offline,
            Self::ConfigError(_) => ErrorCode::ConfigError,
            Self::Io(_) => ErrorCode::Io,
            Self::Http(_) => ErrorCode::Http,
            Self::AuthError(_) => ErrorCode::AuthError,
        }
    }

    pub fn exit_code(&self) -> i32 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorCode, IdxError};

    #[test]
    fn display_and_code_work() {
        let err = IdxError::SymbolNotFound("BBCA".to_string());
        assert_eq!(err.to_string(), "symbol not found: BBCA");
        assert_eq!(err.code(), ErrorCode::SymbolNotFound);

        let parse = IdxError::ParseError("bad json".to_string());
        assert_eq!(parse.code(), ErrorCode::ParseError);

        let unsupported = IdxError::Unsupported("history unavailable".to_string());
        assert_eq!(unsupported.code(), ErrorCode::Unsupported);
    }
}
