//! Errors in the format of the AWS services (doc 39).
//!
//! An AWS error has a **code** the SDK turns into an exception, an HTTP status, a message and who is to blame
//! (`Sender` or `Receiver`). The JSON protocol names the code `com.amazonaws.sqs#QueueDoesNotExist` and, for the SDKs that
//! still read the Query names, adds them in the `x-amzn-query-error` header; the Query protocol sends the Query name
//! in the XML. [`AwsError`] holds both names, and each protocol writes its own.

use crate::core::error::ErrorCode;

/// An error as an AWS client sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsError {
    /// The code of the JSON protocol (`QueueDoesNotExist`).
    pub json_code: &'static str,
    /// The code of the Query protocol (`AWS.SimpleQueueService.NonExistentQueue`).
    pub query_code: &'static str,
    pub status: u16,
    pub message: String,
    /// `true` when the caller is to blame (`Sender`): the SDK does not retry.
    pub sender_fault: bool,
    /// Seconds the client should wait (from a `Retry-After` of the Taba API).
    pub retry_after: Option<u64>,
}

impl AwsError {
    fn new(
        json_code: &'static str,
        query_code: &'static str,
        status: u16,
        sender_fault: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            json_code,
            query_code,
            status,
            message: message.into(),
            sender_fault,
            retry_after: None,
        }
    }

    pub fn queue_does_not_exist() -> Self {
        Self::new(
            "QueueDoesNotExist",
            "AWS.SimpleQueueService.NonExistentQueue",
            400,
            true,
            "The specified queue does not exist.",
        )
    }

    pub fn queue_name_exists(message: impl Into<String>) -> Self {
        Self::new("QueueNameExists", "QueueAlreadyExists", 400, true, message)
    }

    pub fn invalid_parameter(message: impl Into<String>) -> Self {
        Self::new(
            "InvalidParameterValue",
            "InvalidParameterValue",
            400,
            true,
            message,
        )
    }

    pub fn missing_parameter(name: &str) -> Self {
        Self::new(
            "MissingParameter",
            "MissingParameter",
            400,
            true,
            format!("The request must contain the parameter {name}."),
        )
    }

    pub fn invalid_attribute_name(message: impl Into<String>) -> Self {
        Self::new(
            "InvalidAttributeName",
            "InvalidAttributeName",
            400,
            true,
            message,
        )
    }

    pub fn invalid_attribute_value(message: impl Into<String>) -> Self {
        Self::new(
            "InvalidAttributeValue",
            "InvalidAttributeValue",
            400,
            true,
            message,
        )
    }

    pub fn receipt_handle_is_invalid() -> Self {
        Self::new(
            "ReceiptHandleIsInvalid",
            "ReceiptHandleIsInvalid",
            400,
            true,
            "The input receipt handle is invalid.",
        )
    }

    pub fn message_not_inflight() -> Self {
        Self::new(
            "MessageNotInflight",
            "AWS.SimpleQueueService.MessageNotInflight",
            400,
            true,
            "The message referred to is not in flight.",
        )
    }

    pub fn too_many_entries() -> Self {
        Self::new(
            "TooManyEntriesInBatchRequest",
            "AWS.SimpleQueueService.TooManyEntriesInBatchRequest",
            400,
            true,
            "Maximum number of entries per request are 10.",
        )
    }

    pub fn empty_batch() -> Self {
        Self::new(
            "EmptyBatchRequest",
            "AWS.SimpleQueueService.EmptyBatchRequest",
            400,
            true,
            "There should be at least one entry in the request.",
        )
    }

    pub fn entry_ids_not_distinct() -> Self {
        Self::new(
            "BatchEntryIdsNotDistinct",
            "AWS.SimpleQueueService.BatchEntryIdsNotDistinct",
            400,
            true,
            "Two or more batch entries in the request have the same Id.",
        )
    }

    pub fn invalid_entry_id() -> Self {
        Self::new(
            "InvalidBatchEntryId",
            "AWS.SimpleQueueService.InvalidBatchEntryId",
            400,
            true,
            "A batch entry id can only contain alphanumeric characters, hyphens and underscores, up to 80 characters.",
        )
    }

    pub fn batch_too_long() -> Self {
        Self::new(
            "BatchRequestTooLong",
            "AWS.SimpleQueueService.BatchRequestTooLong",
            400,
            true,
            "The length of all the messages put together is more than the limit.",
        )
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(
            "UnsupportedOperation",
            "AWS.SimpleQueueService.UnsupportedOperation",
            400,
            true,
            message,
        )
    }

    pub fn invalid_action(action: &str) -> Self {
        Self::new(
            "InvalidAction",
            "InvalidAction",
            400,
            true,
            format!("The action or operation requested is invalid: {action}."),
        )
    }

    pub fn access_denied(message: impl Into<String>) -> Self {
        Self::new("AccessDenied", "AccessDenied", 403, true, message)
    }

    pub fn invalid_client_token_id() -> Self {
        Self::new(
            "InvalidClientTokenId",
            "InvalidClientTokenId",
            403,
            true,
            "The security token included in the request is invalid.",
        )
    }

    pub fn signature_does_not_match() -> Self {
        Self::new(
            "SignatureDoesNotMatch",
            "SignatureDoesNotMatch",
            403,
            true,
            "The request signature we calculated does not match the signature you provided.",
        )
    }

    pub fn missing_authentication_token() -> Self {
        Self::new(
            "MissingAuthenticationToken",
            "MissingAuthenticationToken",
            403,
            true,
            "Request is missing Authentication Token.",
        )
    }

    pub fn incomplete_signature(message: impl Into<String>) -> Self {
        Self::new(
            "IncompleteSignature",
            "IncompleteSignature",
            403,
            true,
            message,
        )
    }

    pub fn invalid_message_contents(message: impl Into<String>) -> Self {
        Self::new(
            "InvalidMessageContents",
            "InvalidMessageContents",
            400,
            true,
            message,
        )
    }

    pub fn request_expired() -> Self {
        Self::new(
            "RequestExpired",
            "RequestExpired",
            400,
            true,
            "The request timestamp is too far from the current time.",
        )
    }

    pub fn throttled(retry_after: Option<u64>) -> Self {
        let mut error = Self::new(
            "ThrottlingException",
            "RequestThrottled",
            400,
            true,
            "Rate exceeded.",
        );
        error.retry_after = retry_after;
        error
    }

    pub fn unavailable(message: impl Into<String>, retry_after: Option<u64>) -> Self {
        let mut error = Self::new(
            "ServiceUnavailable",
            "ServiceUnavailable",
            503,
            false,
            message,
        );
        error.retry_after = retry_after;
        error
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("InternalError", "InternalError", 500, false, message)
    }

    // ---- the errors SNS names differently (the codes of its Query protocol) ---------------------------------------------

    /// `InvalidParameter`: what SNS answers for a value it does not accept.
    pub fn sns_invalid_parameter(message: impl Into<String>) -> Self {
        Self::new("InvalidParameter", "InvalidParameter", 400, true, message)
    }

    /// `NotFound`: the topic or the subscription does not exist.
    pub fn sns_not_found(message: impl Into<String>) -> Self {
        Self::new("NotFound", "NotFound", 404, true, message)
    }

    pub fn sns_authorization(message: impl Into<String>) -> Self {
        Self::new(
            "AuthorizationError",
            "AuthorizationError",
            403,
            true,
            message,
        )
    }

    pub fn sns_subscription_limit(message: impl Into<String>) -> Self {
        Self::new(
            "SubscriptionLimitExceeded",
            "SubscriptionLimitExceeded",
            403,
            true,
            message,
        )
    }

    pub fn sns_empty_batch() -> Self {
        Self::new(
            "EmptyBatchRequest",
            "EmptyBatchRequest",
            400,
            true,
            "The batch request doesn't contain any entries.",
        )
    }

    pub fn sns_too_many_entries() -> Self {
        Self::new(
            "TooManyEntriesInBatchRequest",
            "TooManyEntriesInBatchRequest",
            400,
            true,
            "The batch request contains more entries than permissible (10).",
        )
    }

    pub fn sns_entry_ids_not_distinct() -> Self {
        Self::new(
            "BatchEntryIdsNotDistinct",
            "BatchEntryIdsNotDistinct",
            400,
            true,
            "Two or more batch entries in the request have the same Id.",
        )
    }

    pub fn sns_invalid_entry_id() -> Self {
        Self::new(
            "InvalidBatchEntryId",
            "InvalidBatchEntryId",
            400,
            true,
            "The Id of a batch entry in a batch request doesn't abide by the specification.",
        )
    }

    pub fn sns_batch_too_long() -> Self {
        Self::new(
            "BatchRequestTooLong",
            "BatchRequestTooLong",
            400,
            true,
            "The length of all the messages put together is more than the limit.",
        )
    }

    /// Translates an error of the Taba client API for SNS: the topic that does not exist is `NotFound`, and a refusal of
    /// the request is `InvalidParameter`. The rest is the same as for SQS.
    pub fn from_taba_sns(status: u16, code: &str, message: &str, retry_after: Option<u64>) -> Self {
        match ErrorCode::parse(code) {
            Some(ErrorCode::TopicNotFound | ErrorCode::TopicWrongType) => {
                Self::sns_not_found("Topic does not exist")
            }
            Some(
                ErrorCode::AuthForbidden | ErrorCode::AuthOutOfScope | ErrorCode::TopicReadOnly,
            ) => Self::sns_authorization(message),
            Some(ErrorCode::AuthUnauthorized) => Self::invalid_client_token_id(),
            Some(ErrorCode::TopicAlreadyExists) => Self::sns_invalid_parameter(message),
            _ if status == 429 => {
                let mut error = Self::new("Throttled", "Throttled", 429, true, "Rate exceeded.");
                error.retry_after = retry_after;
                error
            }
            _ if status == 404 => Self::sns_not_found(message),
            _ if (400..500).contains(&status) && status != 401 && status != 403 => {
                Self::sns_invalid_parameter(message)
            }
            _ => Self::from_taba(status, code, message, retry_after),
        }
    }

    /// Translates an error of the Taba client API. `NotFound` of a topic is the queue that does not exist; the
    /// caller decides what other codes mean in its context before calling this for the rest.
    pub fn from_taba(status: u16, code: &str, message: &str, retry_after: Option<u64>) -> Self {
        let known = ErrorCode::parse(code);
        match known {
            Some(ErrorCode::TopicNotFound | ErrorCode::TopicWrongType) => {
                Self::queue_does_not_exist()
            }
            Some(ErrorCode::AuthUnauthorized) => Self::invalid_client_token_id(),
            Some(ErrorCode::AuthForbidden | ErrorCode::AuthOutOfScope) => {
                Self::access_denied(message)
            }
            Some(ErrorCode::MessageTooLarge) => Self::invalid_parameter(message),
            Some(ErrorCode::MessageStaleReceipt | ErrorCode::MessageNotFound) => {
                Self::message_not_inflight()
            }
            Some(ErrorCode::TopicReadOnly) => Self::access_denied(message),
            Some(ErrorCode::TopicAlreadyExists) => Self::queue_name_exists(message),
            _ if status == 429 => Self::throttled(retry_after),
            _ if status == 401 => Self::invalid_client_token_id(),
            _ if status == 403 => Self::access_denied(message),
            _ if status == 404 => Self::queue_does_not_exist(),
            _ if (400..500).contains(&status) => Self::invalid_parameter(message),
            _ if status == 502 || status == 503 || status == 504 => {
                Self::unavailable(message, retry_after)
            }
            _ => Self::internal(message),
        }
    }
}

impl std::fmt::Display for AwsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.json_code, self.message)
    }
}

#[cfg(test)]
mod tests;
