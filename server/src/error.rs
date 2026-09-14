use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("import failed: {0}")]
    Import(String),

    #[error("payload too large: {0}")]
    PayloadTooLarge(String),

    #[error("too many requests: {0}")]
    TooManyRequests(String),

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("bad request: {0}")]
    BadRequest(String),
}

/// 命令返回值一律 AppResult<T>，HTTP 层把 AppError 统一序列化为 {kind, message}
pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    /// 前端错误体的 kind 字段（与桌面版历史协议保持一致，新增 unauthorized）。
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "not_found",
            AppError::Validation(_) => "validation",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::Conflict(_) => "conflict",
            AppError::Import(_) => "import",
            AppError::PayloadTooLarge(_) => "payload_too_large",
            AppError::TooManyRequests(_) => "too_many_requests",
            AppError::Sqlx(_) => "sql",
            AppError::Migrate(_) => "migrate",
            AppError::Io(_) => "io",
            AppError::Json(_) => "json",
            AppError::BadRequest(_) => "bad_request",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Validation(_) | AppError::BadRequest(_) => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Import(_) | AppError::PayloadTooLarge(_) => StatusCode::BAD_REQUEST,
            AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Sqlx(_) | AppError::Migrate(_) | AppError::Io(_) | AppError::Json(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "kind": self.kind(),
            "message": self.to_string(),
        });
        (self.status_code(), Json(body)).into_response()
    }
}
