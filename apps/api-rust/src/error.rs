use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// API error with the FastAPI-compatible `{"detail": "..."}` body the frontend displays.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: &str) -> Self {
        Self {
            status,
            message: message.to_owned(),
        }
    }

    pub fn unauthorized(message: &str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn not_found(message: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn conflict(message: &str) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn validation(message: &str) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "detail": self.message }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db) = error {
            match db.kind() {
                sqlx::error::ErrorKind::UniqueViolation => {
                    return Self::conflict("Ya existe un registro con el mismo valor único");
                }
                sqlx::error::ErrorKind::ForeignKeyViolation => {
                    return Self::validation("Referencia inexistente");
                }
                _ => {}
            }
        }
        tracing::error!(?error, "database error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Error interno de base de datos",
        )
    }
}
