use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum AppError {
    Database(String),
    Io(String),
    NotFound(String),
    InvalidInput(String),
    AiError(String),
    JoinError(String),
    VaultLocked,
    VaultAlreadyUnlocked,
    VaultInvalidPassword,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AppError::Database(e) => write!(f, "Database error: {}", e),
            AppError::Io(e) => write!(f, "IO error: {}", e),
            AppError::NotFound(e) => write!(f, "Not found: {}", e),
            AppError::InvalidInput(e) => write!(f, "Invalid input: {}", e),
            AppError::AiError(e) => write!(f, "AI error: {}", e),
            AppError::JoinError(e) => write!(f, "Join error: {}", e),
            AppError::VaultLocked => write!(f, "Vault is locked. Please enter your password."),
            AppError::VaultAlreadyUnlocked => write!(f, "Vault is already unlocked."),
            AppError::VaultInvalidPassword => write!(f, "Invalid master password."),
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Database(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}
