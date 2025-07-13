use std::fmt;

#[derive(Debug)]
pub enum BlockDBError {
    IoError(std::io::Error),
    SerializationError(bincode::Error),
    InvalidData(String),
    BlockchainError(String),
    StorageError(String),
    ApiError(String),
    DuplicateKey(String),
}

impl fmt::Display for BlockDBError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockDBError::IoError(e) => write!(f, "IO Error: {}", e),
            BlockDBError::SerializationError(e) => write!(f, "Serialization Error: {}", e),
            BlockDBError::InvalidData(msg) => write!(f, "Invalid Data: {}", msg),
            BlockDBError::BlockchainError(msg) => write!(f, "Blockchain Error: {}", msg),
            BlockDBError::StorageError(msg) => write!(f, "Storage Error: {}", msg),
            BlockDBError::ApiError(msg) => write!(f, "API Error: {}", msg),
            BlockDBError::DuplicateKey(msg) => write!(f, "Duplicate Key Error: {}", msg),
        }
    }
}

impl std::error::Error for BlockDBError {}

impl From<std::io::Error> for BlockDBError {
    fn from(error: std::io::Error) -> Self {
        BlockDBError::IoError(error)
    }
}

impl From<bincode::Error> for BlockDBError {
    fn from(error: bincode::Error) -> Self {
        BlockDBError::SerializationError(error)
    }
}

impl From<Box<dyn std::error::Error>> for BlockDBError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        BlockDBError::StorageError(error.to_string())
    }
}