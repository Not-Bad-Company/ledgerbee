use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String);

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn bad(msg: impl Into<String>) -> Self {
        ApiError(StatusCode::BAD_REQUEST, msg.into())
    }
    pub fn not_found() -> Self {
        ApiError(StatusCode::NOT_FOUND, "Niet gevonden".into())
    }
    pub fn locked(msg: impl Into<String>) -> Self {
        ApiError(StatusCode::CONFLICT, msg.into())
    }
    pub fn unauthorized() -> Self {
        ApiError(StatusCode::UNAUTHORIZED, "Niet ingelogd".into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.0.is_server_error() {
            tracing::error!(status = %self.0, "{}", self.1);
        }
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

macro_rules! internal {
    ($t:ty) => {
        impl From<$t> for ApiError {
            fn from(e: $t) -> Self {
                ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        }
    };
}
internal!(anyhow::Error);
internal!(rusqlite::Error);
internal!(r2d2::Error);
internal!(std::io::Error);
internal!(tokio::task::JoinError);
