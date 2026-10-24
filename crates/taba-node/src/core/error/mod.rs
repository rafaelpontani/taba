//! The error code catalog (doc 06d, section 8.3.6) and the `ErrorResponse`.
//!
//! Each code has a fixed HTTP status. The status goes only in the HTTP
//! status line, never in the body, and `Retry-After` only in the header (section 8.3.3).

use serde::Serialize;
use utoipa::ToSchema;

macro_rules! catalog {
    ($( $variant:ident => ($code:literal, $status:literal) ),+ $(,)?) => {
        /// Stable error code, in the `group.reason` format.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ErrorCode { $($variant),+ }

        impl ErrorCode {
            /// All the catalog's codes.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self { $(ErrorCode::$variant => $code),+ }
            }

            /// The inverse of `as_str`: `None` for a text that is not a catalog code.
            pub fn parse(code: &str) -> Option<ErrorCode> {
                Self::ALL.iter().copied().find(|c| c.as_str() == code)
            }

            /// The code's fixed HTTP status.
            pub const fn status(self) -> u16 {
                match self { $(ErrorCode::$variant => $status),+ }
            }
        }
    };
}

catalog! {
    // Validation (400)
    ValidationFailed => ("validation.failed", 400),
    ValidationRequired => ("validation.required", 400),
    ValidationUnknownField => ("validation.unknown-field", 400),
    ValidationOutOfRange => ("validation.out-of-range", 400),
    ValidationInvalidName => ("validation.invalid-name", 400),
    ValidationReservedName => ("validation.reserved-name", 400),
    ValidationImmutableField => ("validation.immutable-field", 400),
    ValidationNotApplicable => ("validation.not-applicable", 400),
    ValidationInvalidBody => ("validation.invalid-body", 400),
    ValidationInvalidContentType => ("validation.invalid-content-type", 400),
    ValidationGroupIdRequired => ("validation.group-id-required", 400),
    ValidationInvalidCursor => ("validation.invalid-cursor", 400),
    ValidationMaxBytesAboveHardLimit => ("validation.max-bytes-above-hard-limit", 400),
    // Authentication and authorization (401 and 403)
    AuthUnauthorized => ("auth.unauthorized", 401),
    AuthForbidden => ("auth.forbidden", 403),
    AuthOutOfScope => ("auth.out-of-scope", 403),
    OpsIpNotAllowed => ("ops.ip-not-allowed", 403),
    ClusterNotMember => ("cluster.not-member", 403),
    ClusterNodeFrozen => ("cluster.node-frozen", 403),
    // Not found (404)
    TopicNotFound => ("topic.not-found", 404),
    MessageNotFound => ("message.not-found", 404),
    TokenNotFound => ("token.not-found", 404),
    RouteNotFound => ("route.not-found", 404),
    NodeNotFound => ("node.not-found", 404),
    ClusterOperationUnknown => ("cluster.operation-unknown", 404),
    // Conflito (409)
    TopicAlreadyExists => ("topic.already-exists", 409),
    TopicWrongType => ("topic.wrong-type", 409),
    TopicReadOnly => ("topic.read-only", 409),
    MessageStaleReceipt => ("message.stale-receipt", 409),
    DrainConflict => ("drain.conflict", 409),
    ClusterConflict => ("cluster.conflict", 409),
    // Limits and configuration (413, 415, 421, 422, 429)
    MessageTooLarge => ("message.too-large", 413),
    MediaUnsupported => ("media.unsupported", 415),
    FifoMisdirected => ("fifo.misdirected", 421),
    ClusterWrongNode => ("cluster.wrong-node", 421),
    DistributorTooManyTargets => ("distributor.too-many-targets", 422),
    TopicVolatileLimit => ("topic.volatile-limit", 422),
    ClusterJoinUnreachable => ("cluster.join-unreachable", 422),
    BackupNotConfigured => ("backup.not-configured", 422),
    RateLimitExceeded => ("rate-limit.exceeded", 429),
    // Availability and failures (5xx)
    InternalError => ("internal.error", 500),
    ForwardBadGateway => ("forward.bad-gateway", 502),
    ForwardTimeout => ("forward.timeout", 504),
    ClusterQuorumUnavailable => ("cluster.quorum-unavailable", 503),
    ClusterRegistrationFailed => ("cluster.registration-failed", 503),
    ClusterPruneIncomplete => ("cluster.prune-incomplete", 503),
    ClusterCommitTimeout => ("cluster.commit-timeout", 503),
    ClusterNodeSyncing => ("cluster.node-syncing", 503),
    ClusterNodeDraining => ("cluster.node-draining", 503),
    NodeFailed => ("node.failed", 503),
    FifoPrimaryUnavailable => ("fifo.primary-unavailable", 503),
    DeliveryUnavailable => ("delivery.unavailable", 503),
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// A specific problem inside a validation error (`details`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ErrorDetail {
    /// The field the problem is in (`name`, `permissions[1]`, `query.limit`...).
    pub field: String,
    /// A code of the catalogue ([doc 19](../19-errors-and-limits.md)).
    #[schema(value_type = String, example = "validation.out-of-range")]
    pub code: ErrorCode,
    pub message: String,
}

/// The Taba error. Maps straight to the catalog and becomes the `ErrorResponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabaError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Vec<ErrorDetail>,
    /// Seconds until it is worth trying again (`Retry-After`, only in the header).
    pub retry_after: Option<u64>,
    /// What the caller's token lacked, in a `403 auth.forbidden`: `root` or a permission name.
    pub required: Option<String>,
}

impl TabaError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
            retry_after: None,
            required: None,
        }
    }

    /// Says what the token needed (`root`, or the name of a permission).
    pub fn with_required(mut self, required: impl Into<String>) -> Self {
        self.required = Some(required.into());
        self
    }

    pub fn with_detail(
        mut self,
        field: impl Into<String>,
        code: ErrorCode,
        message: impl Into<String>,
    ) -> Self {
        self.details.push(ErrorDetail {
            field: field.into(),
            code,
            message: message.into(),
        });
        self
    }

    pub fn with_retry_after(mut self, seconds: u64) -> Self {
        self.retry_after = Some(seconds);
        self
    }

    pub fn status(&self) -> u16 {
        self.code.status()
    }

    pub fn route_not_found() -> Self {
        Self::new(
            ErrorCode::RouteNotFound,
            "The route does not exist on this port.",
        )
    }

    pub fn internal() -> Self {
        Self::new(ErrorCode::InternalError, "Unexpected node failure.")
    }

    /// The error body, with the `requestId` from the `X-Request-Id` header.
    pub fn to_response(&self, request_id: &str) -> ErrorResponse {
        ErrorResponse {
            code: self.code,
            message: self.message.clone(),
            request_id: request_id.to_owned(),
            details: self.details.clone(),
            required: self.required.clone(),
        }
    }
}

impl std::fmt::Display for TabaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for TabaError {}

/// Body of every error response (`4xx` and `5xx`) of a JSON route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    /// A stable code of the catalogue, in the `group.reason` format. A client must tolerate codes it does not
    /// know yet: the catalogue only grows.
    #[schema(value_type = String, example = "topic.not-found")]
    pub code: ErrorCode,
    pub message: String,
    /// The same value as the `X-Request-Id` header of the response.
    pub request_id: String,
    /// What is wrong in each field, on a validation error.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ErrorDetail>,
    /// On a `403 auth.forbidden`: what the token lacked, `root` or the name of a permission.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<String>,
}

#[cfg(test)]
mod tests;
