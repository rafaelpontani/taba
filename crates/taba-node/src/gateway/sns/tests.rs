//! The SNS port against a small fake of the client API that remembers its topics (the pieces the other tests of this
//! folder use: [`Fake`], [`ask`]).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::routing::{get, post};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::core::clock::SystemClock;

/// One message the fake received on `POST /topics/{topic}/messages`.
#[derive(Clone, Debug)]
pub struct Published {
    pub topic: String,
    pub content_type: String,
    pub authorization: String,
    pub body: String,
}

/// A client API that keeps topics in memory: create, read, list (paged), patch, delete and publish (a distributor answers
/// with a delivery per queue whose name starts with one of its prefixes).
#[derive(Clone, Default)]
pub struct Fake {
    topics: Arc<Mutex<BTreeMap<String, Value>>>,
    pub published: Arc<Mutex<Vec<Published>>>,
    /// The queue whose copy fails.
    pub failing_copy: Arc<Mutex<Option<String>>>,
    /// The size of a page of `GET /topics` (0 = everything).
    pub page_size: Arc<Mutex<usize>>,
}

impl Fake {
    pub fn queue(&self, name: &str, fifo: bool) {
        self.topics.lock().unwrap().insert(
            name.to_owned(),
            json!({"id": name, "name": name, "type": "queue", "fifo": fifo, "tags": {}}),
        );
    }

    pub fn topic(&self, name: &str) -> Option<Value> {
        self.topics.lock().unwrap().get(name).cloned()
    }

    pub fn published(&self) -> Vec<Published> {
        self.published.lock().unwrap().clone()
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/topics", post(create).get(list))
            .route("/topics/{topic}", get(read).patch(patch).delete(remove))
            .route("/topics/{topic}/messages", post(publish))
            .with_state(self.clone())
    }
}

fn error(status: StatusCode, code: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({"code": code, "message": code, "requestId": "r"})),
    )
}

async fn create(State(fake): State<Fake>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let name = body["name"].as_str().unwrap_or("").to_owned();
    let mut topics = fake.topics.lock().unwrap();
    if topics.contains_key(&name) {
        return error(StatusCode::CONFLICT, "topic.already-exists");
    }
    let mut topic = json!({"id": name, "name": name, "type": body["type"], "tags": body["tags"], "fifo": false});
    if body["type"] == "distributor" {
        topic["prefixes"] = json!([]);
    }
    topics.insert(name, topic.clone());
    (StatusCode::CREATED, Json(topic))
}

async fn list(
    State(fake): State<Fake>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Json<Value> {
    let all: Vec<Value> = fake.topics.lock().unwrap().values().cloned().collect();
    let configured = *fake.page_size.lock().unwrap();
    let size = match configured {
        0 => all.len().max(1),
        n => n,
    };
    let from = query
        .get("cursor")
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(0);
    let items: Vec<Value> = all.iter().skip(from).take(size).cloned().collect();
    let mut page = json!({"items": items});
    if from + size < all.len() {
        page["nextCursor"] = json!((from + size).to_string());
    }
    Json(page)
}

async fn read(State(fake): State<Fake>, Path(name): Path<String>) -> (StatusCode, Json<Value>) {
    match fake.topic(&name) {
        Some(topic) => (StatusCode::OK, Json(topic)),
        None => error(StatusCode::NOT_FOUND, "topic.not-found"),
    }
}

async fn patch(
    State(fake): State<Fake>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut topics = fake.topics.lock().unwrap();
    let Some(topic) = topics.get_mut(&name) else {
        return error(StatusCode::NOT_FOUND, "topic.not-found");
    };
    for field in ["prefixes", "tags"] {
        if !body[field].is_null() {
            topic[field] = body[field].clone();
        }
    }
    (StatusCode::OK, Json(topic.clone()))
}

async fn remove(State(fake): State<Fake>, Path(name): Path<String>) -> StatusCode {
    let removed = fake.topics.lock().unwrap().remove(&name);
    match removed {
        Some(_) => StatusCode::NO_CONTENT,
        None => StatusCode::NOT_FOUND,
    }
}

async fn publish(
    State(fake): State<Fake>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, Json<Value>) {
    let Some(topic) = fake.topic(&name) else {
        return error(StatusCode::NOT_FOUND, "topic.not-found");
    };
    let header = |h: &str| {
        headers
            .get(h)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_owned()
    };
    fake.published.lock().unwrap().push(Published {
        topic: name.clone(),
        content_type: header("content-type"),
        authorization: header("authorization"),
        body,
    });
    if topic["type"] == "queue" {
        return (StatusCode::CREATED, Json(json!({"id": "direct"})));
    }
    let prefixes: Vec<String> = topic["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_owned())
        .collect();
    let failing = fake.failing_copy.lock().unwrap().clone();
    let queues: Vec<String> = fake
        .topics
        .lock()
        .unwrap()
        .values()
        .filter(|t| t["type"] == "queue")
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .filter(|q| prefixes.iter().any(|p| q.starts_with(p.as_str())))
        .collect();
    let deliveries: Vec<Value> = queues
        .iter()
        .map(|q| {
            if failing.as_deref() == Some(q) {
                json!({"topic": q, "error": {"code": "cluster.quorum-unavailable", "message": "no quorum"}})
            } else {
                json!({"topic": q, "id": format!("copy-{q}")})
            }
        })
        .collect();
    (
        StatusCode::OK,
        Json(json!({"matched": queues.len(), "deliveries": deliveries})),
    )
}

pub fn gateway_with(fake: &Fake, credentials: Option<Arc<Credentials>>) -> Router {
    router(Arc::new(SnsState {
        backend: Backend::new(fake.router()),
        credentials: credentials
            .clone()
            .unwrap_or_else(|| Arc::new(Credentials::none())),
        auth_enabled: credentials.is_some(),
        clock: Arc::new(SystemClock),
        max_body: 1024 * 1024,
    }))
}

pub fn gateway(fake: &Fake) -> Router {
    gateway_with(fake, None)
}

fn form_request(form: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(form.to_owned()))
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo("10.0.0.7:4000".parse::<SocketAddr>().unwrap()));
    request
}

/// Posts a form to the SNS port and returns the status and the XML.
pub async fn ask(fake: &Fake, form: &str) -> (StatusCode, String) {
    let response = gateway(fake).oneshot(form_request(form)).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    (parts.status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// The text of the first element with that name, if any.
pub fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(&xml[start..end])
}

pub const TOPIC: &str = "arn:aws:sns:us-east-1:000000000000:orders";

/// UC: UC-GTW-026
#[tokio::test]
async fn an_unknown_action_is_invalid_and_an_unsupported_one_says_so() {
    let fake = Fake::default();
    let (status, xml) = ask(&fake, "Action=Frobnicate").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(element(&xml, "Code"), Some("InvalidAction"), "{xml}");

    let (status, xml) = ask(&fake, "Action=CreatePlatformEndpoint").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(element(&xml, "Code"), Some("InvalidParameter"), "{xml}");
    assert!(xml.contains("not supported by this gateway"), "{xml}");

    let (status, xml) = ask(&fake, "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(element(&xml, "Code"), Some("MissingParameter"), "{xml}");
}

/// UC: UC-GTW-026
#[tokio::test]
async fn an_answer_is_xml_with_the_request_id_in_the_body_and_in_the_header() {
    let fake = Fake::default();
    let response = gateway(&fake)
        .oneshot(form_request("Action=ListTopics"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/xml");
    let header = response.headers()["x-amzn-requestid"]
        .to_str()
        .unwrap()
        .to_owned();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let xml = String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(element(&xml, "RequestId"), Some(header.as_str()), "{xml}");
}

/// UC: UC-GTW-026
#[tokio::test]
async fn a_body_over_the_limit_is_refused() {
    let fake = Fake::default();
    let router = router(Arc::new(SnsState {
        backend: Backend::new(fake.router()),
        credentials: Arc::new(Credentials::none()),
        auth_enabled: false,
        clock: Arc::new(SystemClock),
        max_body: 64,
    }));
    let big = format!("Action=Publish&Message={}", "x".repeat(200));
    let response = router.oneshot(form_request(&big)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("InvalidParameter"));
}

/// UC: UC-GTW-025
#[tokio::test]
async fn with_authentication_the_call_is_signed_for_sns_and_carries_the_token_of_the_key() {
    use crate::gateway::aws::sigv4::{self, Signed};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials.json");
    std::fs::write(&path, r#"{"KEY1":{"secret":"secret-1","token":"tb_a.b"}}"#).unwrap();
    let credentials = Arc::new(Credentials::open(&path).unwrap());
    let fake = Fake::default();
    fake.queue("billing", false);
    let ok = ask(&fake, "Action=CreateTopic&Name=orders").await;
    assert_eq!(ok.0, StatusCode::OK);

    let now = SystemClock.now_millis();
    let amz_date = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(now) * 1_000_000)
        .unwrap()
        .format(time::macros::format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .unwrap();
    let body = format!("Action=Publish&TopicArn={TOPIC}&Message=hi");
    let call = |service: &str| {
        let mut headers = HeaderMap::new();
        headers.insert("host", "gw:9325".parse().unwrap());
        headers.insert("x-amz-date", amz_date.parse().unwrap());
        headers.insert(
            "content-type",
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        let authorization = sigv4::sign(
            "KEY1",
            "secret-1",
            "us-east-1",
            service,
            &amz_date,
            &Signed {
                method: "POST",
                path: "/",
                query: "",
                headers: &headers,
                body: body.as_bytes(),
            },
            &["content-type", "host", "x-amz-date"],
        );
        headers.insert("authorization", authorization.parse().unwrap());
        let mut request = Request::builder().method(Method::POST).uri("/");
        *request.headers_mut().unwrap() = headers;
        let mut request = request.body(Body::from(body.clone())).unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo("10.0.0.7:4000".parse::<SocketAddr>().unwrap()));
        request
    };

    let sqs = gateway_with(&fake, Some(credentials.clone()))
        .oneshot(call("sqs"))
        .await
        .unwrap();
    assert_eq!(
        sqs.status(),
        StatusCode::FORBIDDEN,
        "signed for the wrong service"
    );
    assert!(fake.published().is_empty());

    let sns = gateway_with(&fake, Some(credentials))
        .oneshot(call("sns"))
        .await
        .unwrap();
    assert_eq!(sns.status(), StatusCode::OK);
    assert_eq!(fake.published()[0].authorization, "Bearer tb_a.b");
}
