use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("invalid entity type: {0}")]
    InvalidEntityType(String),

    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    #[error("payload must be a JSON object")]
    PayloadNotObject,
}
