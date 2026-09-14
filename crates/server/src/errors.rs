use actix_web::HttpResponse;
use actix_web::http::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),

    #[error("document not found")]
    NotFound,

    #[error("too many searches at once, try again in a moment")]
    TooManyRequests,

    #[error("unknown host")]
    UnknownHost,

    #[error("internal error")]
    Internal,
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        AppError::BadRequest(msg.into())
    }
}

impl actix_web::ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            AppError::UnknownHost => StatusCode::MISDIRECTED_REQUEST,
            AppError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let mut resp = HttpResponse::build(self.status_code());
        if let AppError::TooManyRequests = self {
            resp.insert_header((actix_web::http::header::RETRY_AFTER, "1"));
        }
        resp.json(serde_json::json!({ "error": self.to_string() }))
    }
}
