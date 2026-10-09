use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("本地数据库操作失败")]
    Database(#[from] rusqlite::Error),
    #[error("无法读写所选文件")]
    Io(#[from] std::io::Error),
    #[error("数据文件不是有效的 JSONL")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Bridge(String),
    #[error("{0}")]
    Ai(String),
}

pub type AppResult<T> = Result<T, AppError>;
