//! The port of the gateway against two fake routers that answer with the name of the service that got the request.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use super::*;

/// A router that answers `<name>:<body>`.
fn echo(name: &'static str) -> Router {
    Router::new().fallback(move |request: Request| async move {
        let body = to_bytes(request.into_body(), usize::MAX).await.unwrap();
        format!("{name}:{}", String::from_utf8_lossy(&body))
    })
}

fn port(sqs: bool, sns: bool) -> Router {
    router(sqs.then(|| echo("sqs")), sns.then(|| echo("sns")), 1024)
}

const SCOPE: &str = "AWS4-HMAC-SHA256 Credential=AKID/20231104/us-east-1/{}/aws4_request, SignedHeaders=host;x-amz-date, Signature=abcd";

async fn send(app: &Router, headers: &[(&str, String)], body: &str) -> (StatusCode, String) {
    let mut request = Request::post("/");
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_owned())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let text = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&text).into_owned())
}

fn signed(service: &str) -> Vec<(&'static str, String)> {
    vec![("authorization", SCOPE.replace("{}", service))]
}

/// UC: UC-GTW-030
#[tokio::test]
async fn a_signed_call_goes_to_the_service_of_its_credential_scope() {
    let app = port(true, true);
    assert_eq!(
        send(&app, &signed("sns"), "Action=Publish").await,
        (StatusCode::OK, "sns:Action=Publish".into())
    );
    assert_eq!(
        send(&app, &signed("sqs"), "{}").await,
        (StatusCode::OK, "sqs:{}".into())
    );
}

/// UC: UC-GTW-030
#[tokio::test]
async fn the_json_protocol_of_sqs_is_known_by_its_target_header() {
    let app = port(true, true);
    let headers = [("x-amz-target", "AmazonSQS.SendMessage".to_owned())];
    assert_eq!(
        send(&app, &headers, "{}").await,
        (StatusCode::OK, "sqs:{}".into())
    );
}

/// UC: UC-GTW-030
#[tokio::test]
async fn an_unsigned_form_goes_to_the_service_of_its_action_and_its_body_is_kept() {
    let app = port(true, true);
    assert_eq!(
        send(&app, &[], "Action=CreateTopic&Name=t").await,
        (StatusCode::OK, "sns:Action=CreateTopic&Name=t".into())
    );
    assert_eq!(
        send(&app, &[], "Action=SendMessage&MessageBody=x").await,
        (
            StatusCode::OK,
            "sqs:Action=SendMessage&MessageBody=x".into()
        )
    );
    assert_eq!(
        send(&app, &[], "").await,
        (StatusCode::OK, "sqs:".into()),
        "what has no action is for SQS, which says what is wrong"
    );
}

/// UC: UC-GTW-030
#[tokio::test]
async fn an_unsigned_call_for_a_service_that_is_off_is_told_so() {
    let (status, body) = send(&port(false, true), &[], "Action=SendMessage").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("gateway.sqs.enabled"), "{body}");
    let (status, body) = send(&port(true, false), &[], "Action=Publish").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("gateway.sns.enabled"), "{body}");
    assert_eq!(
        send(&port(true, false), &[], "Action=SendMessage").await,
        (StatusCode::OK, "sqs:Action=SendMessage".into())
    );
}

/// UC: UC-GTW-030
#[tokio::test]
async fn a_service_that_is_off_answers_with_the_key_that_turns_it_on() {
    let (status, body) = send(&port(true, false), &signed("sns"), "Action=Publish").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("gateway.sns.enabled"), "{body}");
    let (status, body) = send(&port(false, true), &signed("sqs"), "{}").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("gateway.sqs.enabled"), "{body}");
}

/// UC: UC-GTW-030
#[tokio::test]
async fn a_body_over_the_limit_is_left_to_the_sqs_handler() {
    let app = router(Some(echo("sqs")), Some(echo("sns")), 8);
    let (status, body) = send(&app, &[], "Action=Publish&Message=more-than-eight-bytes").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.starts_with("sqs:"), "{body}");
}

/// UC: UC-GTW-030
#[test]
fn every_action_of_sns_the_port_knows_is_a_distinct_name() {
    let mut names = sns::ACTIONS.to_vec();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), sns::ACTIONS.len());
    for sqs_only in ["SendMessage", "ReceiveMessage", "CreateQueue", "TagQueue"] {
        assert!(!sns::ACTIONS.contains(&sqs_only), "{sqs_only}");
    }
}
