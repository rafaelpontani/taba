use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, Path, RawQuery, State};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::routing::{get, post};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::core::clock::SystemClock;
use crate::gateway::aws::credentials::Credentials;
use crate::gateway::aws::sigv4;

/// What the fake client API saw.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

impl Seen {
    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

async fn publish(
    State(seen): State<Seen>,
    Path(topic): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, Json<Value>) {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_owned()
    };
    seen.0.lock().unwrap().push(format!(
        "POST /topics/{topic}/messages?{} [{}] [{}] {body}",
        query.unwrap_or_default(),
        header("content-type"),
        header("authorization"),
    ));
    (StatusCode::CREATED, Json(json!({"id": "m1"})))
}

async fn consume() -> Json<Value> {
    Json(json!({"messages": [{
        "id": "msg-1", "receiptToken": "rt-1", "body": "hello", "contentType": "text/plain",
        "deliveryCount": 2, "publishedAt": "2026-11-04T10:00:00.000Z", "invisibleUntil": "2026-11-04T10:00:30.000Z"
    }]}))
}

async fn topic(Path(name): Path<String>) -> (StatusCode, Json<Value>) {
    if name == "orders" {
        (
            StatusCode::OK,
            Json(
                json!({"id": "t", "name": "orders", "type": "queue", "tags": {}, "createdAt": "2026-11-04T09:00:00.000Z",
                "effective": {"maxMessageBytes": 5_242_880, "ttl": 1_209_600, "maxDeliveries": 10}}),
            ),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"code": "topic.not-found", "message": "no", "requestId": "r"})),
        )
    }
}

fn client(seen: &Seen) -> Router {
    Router::new()
        .route("/topics/{topic}", get(topic))
        .route("/topics/{topic}/messages", post(publish).get(consume))
        .with_state(seen.clone())
}

fn gateway(seen: &Seen, credentials: Option<Arc<Credentials>>) -> Router {
    router(Arc::new(SqsState::new(
        Backend::new(client(seen)),
        credentials
            .clone()
            .unwrap_or_else(|| Arc::new(Credentials::none())),
        credentials.is_some(),
        Some("http://gw.test:9324".into()),
        Arc::new(SystemClock),
        16 * 1024 * 1024,
    )))
}

fn request(builder: axum::http::request::Builder, body: &str) -> Request<Body> {
    let mut request = builder.body(Body::from(body.to_owned())).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo("10.0.0.7:4000".parse::<SocketAddr>().unwrap()));
    request
}

fn json_call(action: &str, body: &str) -> Request<Body> {
    request(
        Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("x-amz-target", format!("AmazonSQS.{action}"))
            .header("content-type", "application/x-amz-json-1.0"),
        body,
    )
}

async fn answer(router: Router, request: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let response = router.oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    (
        parts.status,
        parts.headers,
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

const URL: &str = "http://gw.test:9324/000000000000/orders";

/// UC: UC-GTW-013
#[tokio::test]
async fn a_send_over_the_json_protocol_publishes_plain_text_and_answers_the_md5() {
    let seen = Seen::default();
    let (status, headers, body) = answer(
        gateway(&seen, None),
        json_call(
            "SendMessage",
            &json!({"QueueUrl": URL, "MessageBody": "hello world"}).to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(headers["content-type"], "application/x-amz-json-1.0");
    let result: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(result["MessageId"], "m1");
    assert_eq!(
        result["MD5OfMessageBody"],
        "5eb63bbbe01eeed093cb22bb8f5acdc3"
    );
    assert_eq!(
        seen.all(),
        vec!["POST /topics/orders/messages? [text/plain; charset=utf-8] [-] hello world"]
    );
}

/// UC: UC-GTW-013
#[tokio::test]
async fn a_send_over_the_query_protocol_answers_xml() {
    let seen = Seen::default();
    let (status, headers, body) = answer(
        gateway(&seen, None),
        request(
            Request::builder()
                .method(Method::POST)
                .uri("/000000000000/orders")
                .header("content-type", "application/x-www-form-urlencoded"),
            "Action=SendMessage&Version=2012-11-05&MessageBody=hello+world&DelaySeconds=5",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(headers["content-type"], "text/xml");
    assert!(body.contains("<SendMessageResult>"), "{body}");
    assert!(body.contains("<MessageId>m1</MessageId>"), "{body}");
    assert!(
        body.contains("<MD5OfMessageBody>5eb63bbbe01eeed093cb22bb8f5acdc3</MD5OfMessageBody>"),
        "{body}"
    );
    // The queue came from the path, and the delay went on as `delaySeconds`.
    assert_eq!(
        seen.all(),
        vec![
            "POST /topics/orders/messages?delaySeconds=5 [text/plain; charset=utf-8] [-] hello world"
        ]
    );
}

/// UC: UC-GTW-013
#[tokio::test]
async fn a_message_with_attributes_is_published_as_the_envelope() {
    let seen = Seen::default();
    let (status, _, body) = answer(
        gateway(&seen, None),
        json_call(
            "SendMessage",
            &json!({"QueueUrl": URL, "MessageBody": "b",
                "MessageAttributes": {"color": {"DataType": "String", "StringValue": "blue"}}})
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(serde_json::from_str::<Value>(&body).unwrap()["MD5OfMessageAttributes"].is_string());
    let call = &seen.all()[0];
    assert!(call.contains("[application/json]"), "{call}");
    assert!(call.contains("\"tabaSqs\":1"), "{call}");
}

/// UC: UC-GTW-013
#[tokio::test]
async fn a_receive_gives_the_handle_the_count_and_the_sent_time() {
    let seen = Seen::default();
    let (status, _, body) = answer(
        gateway(&seen, None),
        json_call(
            "ReceiveMessage",
            &json!({"QueueUrl": URL, "AttributeNames": ["All"], "VisibilityTimeout": 30, "WaitTimeSeconds": 0})
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let message = &serde_json::from_str::<Value>(&body).unwrap()["Messages"][0];
    assert_eq!(message["Body"], "hello");
    assert_eq!(message["MD5OfBody"], "5d41402abc4b2a76b9719d911017c592");
    assert_eq!(message["Attributes"]["ApproximateReceiveCount"], "2");
    assert_eq!(message["Attributes"]["SentTimestamp"], "1793786400000");
    let (id, token) = receipt::decode(message["ReceiptHandle"].as_str().unwrap()).unwrap();
    assert_eq!((id.as_str(), token.as_str()), ("msg-1", "rt-1"));
}

/// UC: UC-GTW-013
#[tokio::test]
async fn an_error_of_the_queue_is_answered_in_the_protocol_of_the_client() {
    let seen = Seen::default();
    let missing = "http://gw.test:9324/000000000000/nope";
    let (status, headers, body) = answer(
        gateway(&seen, None),
        json_call(
            "GetQueueAttributes",
            &json!({"QueueUrl": missing, "AttributeNames": ["All"]}).to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(error["__type"], "com.amazonaws.sqs#QueueDoesNotExist");
    assert_eq!(
        headers["x-amzn-query-error"],
        "AWS.SimpleQueueService.NonExistentQueue;Sender"
    );

    let (status, _, body) = answer(
        gateway(&seen, None),
        request(
            Request::builder()
                .method(Method::POST)
                .uri("/")
                .header("content-type", "application/x-www-form-urlencoded"),
            &format!("Action=GetQueueAttributes&QueueUrl={missing}&AttributeName.1=All"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("<Code>AWS.SimpleQueueService.NonExistentQueue</Code>"),
        "{body}"
    );
}

/// UC: UC-GTW-013
#[tokio::test]
async fn an_unknown_action_is_refused() {
    let (status, _, body) = answer(
        gateway(&Seen::default(), None),
        json_call("Frobnicate", "{}"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("InvalidAction"), "{body}");
    let (_, _, body) = answer(
        gateway(&Seen::default(), None),
        json_call("AddPermission", "{}"),
    )
    .await;
    assert!(body.contains("UnsupportedOperation"), "{body}");
}

fn credentials() -> Arc<Credentials> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aws.json");
    std::fs::write(
        &path,
        r#"{"AKIAOK":{"secret":"the-secret","token":"tb_ok.token"}}"#,
    )
    .unwrap();
    let credentials = Credentials::open(&path).unwrap();
    // The file is read once at open; the folder can go.
    Arc::new(credentials)
}

fn signed_call(access_key: &str, secret: &str, body: &str) -> Request<Body> {
    let amz_date = time::OffsetDateTime::now_utc()
        .format(time::macros::format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("host", "gw.test:9324".parse().unwrap());
    headers.insert("x-amz-date", amz_date.parse().unwrap());
    headers.insert("x-amz-target", "AmazonSQS.SendMessage".parse().unwrap());
    headers.insert(
        "content-type",
        "application/x-amz-json-1.0".parse().unwrap(),
    );
    let authorization = sigv4::sign(
        access_key,
        secret,
        "eu-west-1",
        "sqs",
        &amz_date,
        &sigv4::Signed {
            method: "POST",
            path: "/",
            query: "",
            headers: &headers,
            body: body.as_bytes(),
        },
        &["content-type", "host", "x-amz-date", "x-amz-target"],
    );
    request(
        Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("host", "gw.test:9324")
            .header("x-amz-date", &amz_date)
            .header("x-amz-target", "AmazonSQS.SendMessage")
            .header("content-type", "application/x-amz-json-1.0")
            .header("authorization", authorization),
        body,
    )
}

/// UC: UC-GTW-014
#[tokio::test]
async fn with_authentication_on_a_signed_call_runs_with_the_token_of_the_key() {
    let seen = Seen::default();
    let body = json!({"QueueUrl": URL, "MessageBody": "signed"}).to_string();
    let (status, _, text) = answer(
        gateway(&seen, Some(credentials())),
        signed_call("AKIAOK", "the-secret", &body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(
        seen.all(),
        vec![
            "POST /topics/orders/messages? [text/plain; charset=utf-8] [Bearer tb_ok.token] signed"
        ]
    );
}

/// UC: UC-GTW-014
#[tokio::test]
async fn with_authentication_on_a_wrong_secret_an_unknown_key_or_no_signature_is_refused() {
    let seen = Seen::default();
    let body = json!({"QueueUrl": URL, "MessageBody": "x"}).to_string();
    let expect = |text: &str, code: &str| assert!(text.contains(code), "{text}");

    let (status, _, text) = answer(
        gateway(&seen, Some(credentials())),
        signed_call("AKIAOK", "another-secret", &body),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    expect(&text, "SignatureDoesNotMatch");

    let (status, _, text) = answer(
        gateway(&seen, Some(credentials())),
        signed_call("AKIANO", "the-secret", &body),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    expect(&text, "InvalidClientTokenId");

    let (status, _, text) = answer(
        gateway(&seen, Some(credentials())),
        json_call("SendMessage", &body),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    expect(&text, "MissingAuthenticationToken");

    assert!(seen.all().is_empty(), "nothing reached the client API");
}

/// UC: UC-GTW-014
#[tokio::test]
async fn a_body_changed_after_signing_is_refused() {
    let seen = Seen::default();
    let signed = signed_call(
        "AKIAOK",
        "the-secret",
        &json!({"QueueUrl": URL, "MessageBody": "original"}).to_string(),
    );
    let (parts, _) = signed.into_parts();
    let mut tampered = Request::from_parts(
        parts,
        Body::from(json!({"QueueUrl": URL, "MessageBody": "tampered"}).to_string()),
    );
    tampered
        .extensions_mut()
        .insert(ConnectInfo("10.0.0.7:4000".parse::<SocketAddr>().unwrap()));
    let (status, _, text) = answer(gateway(&seen, Some(credentials())), tampered).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(text.contains("SignatureDoesNotMatch"), "{text}");
    assert!(seen.all().is_empty());
}

/// UC: UC-GTW-013
#[test]
fn the_parameters_are_read_as_numbers_or_texts() {
    let params = json!({"A": 5, "B": "7", "C": "x", "D": "", "E": ["a", "b"], "F": {"k": "v"}});
    assert_eq!(number(&params, "A").unwrap(), Some(5));
    assert_eq!(number(&params, "B").unwrap(), Some(7));
    assert_eq!(number(&params, "D").unwrap(), None);
    assert_eq!(number(&params, "Z").unwrap(), None);
    assert!(number(&params, "C").is_err());
    assert!(number_in(&params, "A", 6, 9).is_err());
    assert_eq!(number_in(&params, "A", 1, 9).unwrap(), Some(5));
    assert_eq!(texts(&params, "E"), vec!["a", "b"]);
    assert_eq!(
        text_map(&params, "F"),
        vec![("k".to_owned(), "v".to_owned())]
    );
    assert_eq!(text(&params, "D"), None);
    assert_eq!(url_encode("a b/c=d~e"), "a%20b%2Fc%3Dd~e");
}
