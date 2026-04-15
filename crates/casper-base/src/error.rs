use axum::http::StatusCode;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum CasperError {
    #[error("Not authenticated")]
    Unauthorized,

    #[error("Insufficient permissions: {0}")]
    Forbidden(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Rate limited")]
    RateLimited,

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Service unavailable: {0}")]
    Unavailable(String),

    #[error("Bad gateway: {0}")]
    BadGateway(String),

    #[error("Gateway timeout: {0}")]
    GatewayTimeout(String),
}

impl CasperError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::BadGateway(_) => StatusCode::BAD_GATEWAY,
            Self::GatewayTimeout(_) => StatusCode::GATEWAY_TIMEOUT,
        }
    }

    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::BadRequest(_) => "bad_request",
            Self::RateLimited => "rate_limited",
            Self::Internal(_) => "internal",
            Self::Unavailable(_) => "unavailable",
            Self::BadGateway(_) => "bad_gateway",
            Self::GatewayTimeout(_) => "gateway_timeout",
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "error": self.variant_name(),
            "message": self.to_string()
        })
    }
}

impl axum::response::IntoResponse for CasperError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code();
        let body = axum::Json(self.to_json());
        (status, body).into_response()
    }
}
