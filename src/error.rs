use rmcp::ErrorData;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, LiteCodeError>;

#[derive(Debug, Error)]
pub enum LiteCodeError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Internal(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl LiteCodeError {
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

impl From<LiteCodeError> for ErrorData {
    fn from(value: LiteCodeError) -> Self {
        match value {
            LiteCodeError::InvalidInput(message) => ErrorData::invalid_params(message, None),
            LiteCodeError::Internal(message) => ErrorData::internal_error(message, None),
            LiteCodeError::Io(error) => ErrorData::internal_error(error.to_string(), None),
        }
    }
}

impl From<rmcp::service::ServerInitializeError> for LiteCodeError {
    fn from(value: rmcp::service::ServerInitializeError) -> Self {
        Self::internal(value.to_string())
    }
}

impl From<tokio::task::JoinError> for LiteCodeError {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::internal(value.to_string())
    }
}
