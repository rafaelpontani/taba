//! O catálogo de códigos de erro (doc 06d, seção 8.3.6) e o `ErrorResponse`.
//!
//! Cada código tem um status HTTP fixo. O status vai só na linha de status do
//! HTTP, nunca no corpo, e o `Retry-After` só no cabeçalho (seção 8.3.3).

use serde::Serialize;

macro_rules! catalog {
    ($( $variant:ident => ($code:literal, $status:literal) ),+ $(,)?) => {
        /// Código estável de erro, no formato `grupo.motivo`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ErrorCode { $($variant),+ }

        impl ErrorCode {
            /// Todos os códigos do catálogo.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self { $(ErrorCode::$variant => $code),+ }
            }

            /// O inverso de `as_str`: `None` para um texto que não é código do catálogo.
            pub fn parse(code: &str) -> Option<ErrorCode> {
                Self::ALL.iter().copied().find(|c| c.as_str() == code)
            }

            /// O status HTTP fixo do código.
            pub const fn status(self) -> u16 {
                match self { $(ErrorCode::$variant => $status),+ }
            }
        }
    };
}

catalog! {
    // Validação (400)
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
    // Autenticação e autorização (401 e 403)
    AuthUnauthorized => ("auth.unauthorized", 401),
    AuthForbidden => ("auth.forbidden", 403),
    AuthOutOfScope => ("auth.out-of-scope", 403),
    OpsIpNotAllowed => ("ops.ip-not-allowed", 403),
    ClusterNotMember => ("cluster.not-member", 403),
    ClusterNodeFrozen => ("cluster.node-frozen", 403),
    // Não encontrado (404)
    TopicNotFound => ("topic.not-found", 404),
    MessageNotFound => ("message.not-found", 404),
    TokenNotFound => ("token.not-found", 404),
    RouteNotFound => ("route.not-found", 404),
    ClusterOperationUnknown => ("cluster.operation-unknown", 404),
    // Conflito (409)
    TopicAlreadyExists => ("topic.already-exists", 409),
    TopicWrongType => ("topic.wrong-type", 409),
    TopicReadOnly => ("topic.read-only", 409),
    MessageStaleReceipt => ("message.stale-receipt", 409),
    DrainConflict => ("drain.conflict", 409),
    ClusterConflict => ("cluster.conflict", 409),
    // Limites e configuração (413, 415, 421, 422, 429)
    MessageTooLarge => ("message.too-large", 413),
    MediaUnsupported => ("media.unsupported", 415),
    FifoMisdirected => ("fifo.misdirected", 421),
    ClusterWrongNode => ("cluster.wrong-node", 421),
    TopicFifoNeedsReplication => ("topic.fifo-needs-replication", 422),
    DistributorTooManyTargets => ("distributor.too-many-targets", 422),
    TopicVolatileLimit => ("topic.volatile-limit", 422),
    ClusterJoinUnreachable => ("cluster.join-unreachable", 422),
    RateLimitExceeded => ("rate-limit.exceeded", 429),
    // Disponibilidade e falhas (5xx)
    InternalError => ("internal.error", 500),
    ForwardBadGateway => ("forward.bad-gateway", 502),
    ForwardTimeout => ("forward.timeout", 504),
    ClusterQuorumUnavailable => ("cluster.quorum-unavailable", 503),
    ClusterRegistrationFailed => ("cluster.registration-failed", 503),
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

/// Um problema pontual dentro de um erro de validação (`details`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorDetail {
    pub field: String,
    pub code: ErrorCode,
    pub message: String,
}

/// O erro do Taba. Mapeia direto para o catálogo e vira o `ErrorResponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabaError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Vec<ErrorDetail>,
    /// Segundos até valer a pena tentar de novo (`Retry-After`, só no cabeçalho).
    pub retry_after: Option<u64>,
}

impl TabaError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
            retry_after: None,
        }
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
        Self::new(ErrorCode::RouteNotFound, "A rota não existe nesta porta.")
    }

    pub fn internal() -> Self {
        Self::new(ErrorCode::InternalError, "Falha inesperada do nó.")
    }

    /// O corpo de erro, com o `requestId` do cabeçalho `X-Request-Id`.
    pub fn to_response(&self, request_id: &str) -> ErrorResponse {
        ErrorResponse {
            code: self.code,
            message: self.message.clone(),
            request_id: request_id.to_owned(),
            details: self.details.clone(),
        }
    }
}

impl std::fmt::Display for TabaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for TabaError {}

/// Corpo de toda resposta de erro (`4xx` e `5xx`) de uma rota JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub code: ErrorCode,
    pub message: String,
    pub request_id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ErrorDetail>,
}

#[cfg(test)]
mod tests;
