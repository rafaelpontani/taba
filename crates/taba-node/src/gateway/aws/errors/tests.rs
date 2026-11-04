use super::*;

/// UC: UC-GTW-003
#[test]
fn a_topic_that_does_not_exist_is_a_queue_that_does_not_exist() {
    let error = AwsError::from_taba(404, "topic.not-found", "no topic", None);
    assert_eq!(error.json_code, "QueueDoesNotExist");
    assert_eq!(error.query_code, "AWS.SimpleQueueService.NonExistentQueue");
    assert!(error.sender_fault);
}

/// UC: UC-GTW-003
#[test]
fn permission_errors_are_access_denied() {
    for code in ["auth.forbidden", "auth.out-of-scope"] {
        let error = AwsError::from_taba(403, code, "no", None);
        assert_eq!(error.json_code, "AccessDenied");
        assert_eq!(error.status, 403);
    }
    assert_eq!(
        AwsError::from_taba(401, "auth.unauthorized", "no", None).json_code,
        "InvalidClientTokenId"
    );
}

/// UC: UC-GTW-003
#[test]
fn overload_and_cluster_errors_tell_the_client_to_retry() {
    let throttled = AwsError::from_taba(429, "rate-limit.exceeded", "slow down", Some(3));
    assert_eq!(throttled.json_code, "ThrottlingException");
    assert_eq!(throttled.query_code, "RequestThrottled");
    assert_eq!(throttled.retry_after, Some(3));
    let unavailable = AwsError::from_taba(503, "cluster.node-draining", "drain", Some(5));
    assert_eq!(unavailable.status, 503);
    assert!(
        !unavailable.sender_fault,
        "the SDK retries a receiver fault"
    );
    assert_eq!(unavailable.retry_after, Some(5));
}

/// UC: UC-GTW-026
#[test]
fn for_sns_a_topic_that_does_not_exist_is_not_found_and_a_refused_request_is_an_invalid_parameter()
{
    let missing = AwsError::from_taba_sns(404, "topic.not-found", "no topic", None);
    assert_eq!(missing.query_code, "NotFound");
    assert_eq!(missing.status, 404);
    assert!(missing.sender_fault);
    assert_eq!(
        AwsError::from_taba_sns(409, "topic.wrong-type", "queue", None).query_code,
        "NotFound"
    );
    let refused = AwsError::from_taba_sns(400, "validation.out-of-range", "bad", None);
    assert_eq!(refused.query_code, "InvalidParameter");
    assert_eq!(refused.status, 400);
    assert_eq!(
        AwsError::from_taba_sns(422, "distributor.too-many-targets", "too many", None).query_code,
        "InvalidParameter"
    );
}

/// UC: UC-GTW-026
#[test]
fn for_sns_permission_throttling_and_cluster_errors_keep_their_meaning() {
    let denied = AwsError::from_taba_sns(403, "auth.forbidden", "no", None);
    assert_eq!(denied.query_code, "AuthorizationError");
    assert_eq!(denied.status, 403);
    assert_eq!(
        AwsError::from_taba_sns(401, "auth.unauthorized", "no", None).query_code,
        "InvalidClientTokenId"
    );
    let throttled = AwsError::from_taba_sns(429, "rate-limit.exceeded", "slow", Some(2));
    assert_eq!(throttled.query_code, "Throttled");
    assert_eq!(throttled.retry_after, Some(2));
    let unavailable = AwsError::from_taba_sns(503, "cluster.node-draining", "drain", Some(5));
    assert_eq!(unavailable.status, 503);
    assert!(
        !unavailable.sender_fault,
        "the SDK retries a receiver fault"
    );
}

/// UC: UC-GTW-003
#[test]
fn a_validation_error_is_an_invalid_parameter_and_an_unknown_one_is_internal() {
    assert_eq!(
        AwsError::from_taba(400, "validation.out-of-range", "bad", None).json_code,
        "InvalidParameterValue"
    );
    assert_eq!(
        AwsError::from_taba(500, "internal.error", "boom", None).json_code,
        "InternalError"
    );
}
