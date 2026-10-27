//! The one error type handlers return. It renders as the protocol's `ApiErrorBody`.

use std::collections::BTreeMap;

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use holocron_protocol::errors::{
    ApiErrorBody, ErrorCode, FieldError, FieldErrorCode, OwnedResource,
};
use holocron_storage::ReadError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    body: ApiErrorBody,
}

impl ApiError {
    pub fn new(code: ErrorCode) -> Self {
        Self::with_message(code, default_message(code))
    }

    pub fn with_message(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            body: ApiErrorBody {
                error_code: code,
                message: message.into(),
                errors: None,
                current: None,
                owned: None,
                expected_origin: None,
            },
        }
    }

    /// Missing, hidden and unknown all answer with exactly this value.
    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound)
    }

    pub fn stale_revision(current: i64) -> Self {
        let mut error = Self::new(ErrorCode::StaleRevision);
        error.body.current = Some(current);
        error
    }

    /// `users.delete` refused while the user owns these (decision 0007).
    pub fn user_owns_resources(owned: Vec<OwnedResource>) -> Self {
        let mut error = Self::new(ErrorCode::UserOwnsResources);
        error.body.owned = Some(owned);
        error
    }

    /// A command from the wrong origin. Names the configured public origin so a sign-in page
    /// can tell where to open Holocron instead.
    pub fn origin_rejected(expected_origin: Option<&str>) -> Self {
        let mut error = Self::new(ErrorCode::OriginRejected);
        error.body.expected_origin = expected_origin.map(Box::from);
        error
    }

    /// A handler stub whose behavior is not built yet. Answers `internal` and logs the name.
    pub fn not_implemented(what: &str) -> Self {
        Self::internal(format!("not implemented: {what}"))
    }

    /// `code` naming one field, for refusals beyond `invalid_arguments` (decision 0004).
    pub fn field(
        code: ErrorCode,
        field: &str,
        error: FieldError,
        message: impl Into<String>,
    ) -> Self {
        let mut answer = Self::with_message(code, message);
        answer.body.errors = Some(BTreeMap::from([(field.to_owned(), vec![error])]));
        answer
    }

    /// Logs the cause and answers a body without internals.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        tracing::error!(error = %cause, "internal error");
        Self::new(ErrorCode::Internal)
    }

    pub fn code(&self) -> ErrorCode {
        self.body.error_code
    }

    pub fn body(&self) -> &ApiErrorBody {
        &self.body
    }
}

fn default_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthenticated => "Sign in to continue.",
        ErrorCode::InvalidCredentials => "Email or password is wrong.",
        ErrorCode::SecondFactorRequired => "Enter the code from your authenticator app.",
        ErrorCode::SecondFactorInvalid => "The code is wrong or was already used.",
        ErrorCode::SecondFactorEnrollmentRequired => {
            "Set up two-factor authentication to continue."
        }
        ErrorCode::ReauthenticationRequired => "Confirm your password to continue.",
        ErrorCode::OperationNotGranted => "The API key does not hold this operation.",
        ErrorCode::GrantExceedsPrincipal => "A key or share cannot grant more than you hold.",
        ErrorCode::ShareScopeExceeded => "The request reads outside what the share allows.",
        ErrorCode::UserOwnsResources => "Transfer or delete what the user owns first.",
        ErrorCode::LastInstallAdmin => "The installation needs at least one admin.",
        ErrorCode::NotFound => "Not found.",
        ErrorCode::Forbidden => "Your role does not allow this.",
        ErrorCode::OriginRejected => {
            "Commands need the request marker and a same-origin Origin header."
        }
        ErrorCode::StaleRevision => "The resource changed since it was read.",
        ErrorCode::IdempotencyConflict => "The idempotency key was used for a different request.",
        ErrorCode::MalformedRequest => "The request does not parse.",
        ErrorCode::InvalidArguments => "Some arguments are invalid.",
        ErrorCode::PayloadTooLarge => "The request is too large.",
        ErrorCode::RateLimited => "Too many attempts. Try again in a minute.",
        ErrorCode::QueryTimeout => "The query took too long and was cancelled.",
        ErrorCode::Internal => "Internal error.",
        _ => "The request was refused.",
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

impl From<ReadError> for ApiError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::Timeout => Self::new(ErrorCode::QueryTimeout),
            ReadError::Busy => Self::with_message(
                ErrorCode::RateLimited,
                "Too many reads are waiting. Try again shortly.",
            ),
            other => Self::internal(other),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.body.error_code.status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self.body)).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

/// Collects field errors; [`FieldErrors::finish`] turns any into `invalid_arguments`.
#[derive(Default)]
pub struct FieldErrors(BTreeMap<String, Vec<FieldError>>);

impl FieldErrors {
    pub fn add(&mut self, field: &str, code: FieldErrorCode, limit: Option<u32>) {
        self.0
            .entry(field.to_owned())
            .or_default()
            .push(FieldError { code, limit });
    }

    /// `invalid_arguments` naming one field.
    pub fn one(field: &str, code: FieldErrorCode, limit: Option<u32>) -> ApiError {
        let mut errors = Self::default();
        errors.add(field, code, limit);
        errors.finish().expect_err("one field error")
    }

    pub fn finish(self) -> Result<(), ApiError> {
        if self.0.is_empty() {
            return Ok(());
        }
        let mut error = ApiError::new(ErrorCode::InvalidArguments);
        error.body.errors = Some(self.0);
        Err(error)
    }
}
