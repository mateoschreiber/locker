use axum::extract::{FromRequest, Request};
use serde_json::Value;

use crate::error::ApiError;

/// JSON body extractor that reports parse errors as `422 {"detail": ...}`
/// instead of axum's default plain-text 400 (FastAPI parity for bad payloads).
pub struct ValidJson(pub Value);

impl<S: Send + Sync> FromRequest<S> for ValidJson {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<Value>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(ValidJson(value)),
            Err(rejection) => Err(ApiError::validation(&format!("JSON inválido: {rejection}"))),
        }
    }
}
