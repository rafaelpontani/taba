//! The S3 client (`S3Store`, over `aws-sdk-s3`) against a fake S3 over HTTP: the real SDK
//! builds the requests (path, `Range`, `If-None-Match`, `If-Match`, `ListObjectsV2`) and
//! reads the responses (`ETag`, `412`, `404 NoSuchKey`, listing XML). It is what MinIO and AWS do
//! on the other side; the test holds for the contract the backup uses from the store (`ObjectStore`).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use taba_node::storage::backup::{ObjectStore, Prefixed, S3Settings, S3Store};

#[derive(Default)]
struct Bucket {
    objects: BTreeMap<String, (Vec<u8>, String)>,
    version: u64,
    /// The headers of each request the SDK sent (to check what it asks for).
    seen: Vec<String>,
}

type Shared = Arc<Mutex<Bucket>>;

fn xml_error(status: StatusCode, code: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/xml")],
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>{code}</Code><Message>{code}</Message></Error>"
        ),
    )
        .into_response()
}

async fn put_object(
    State(bucket): State<Shared>,
    Path((_bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut bucket = bucket.lock().unwrap();
    bucket.seen.push(format!("PUT {key}"));
    let current = bucket.objects.get(&key).map(|(_, etag)| etag.clone());
    if headers
        .get("if-none-match")
        .is_some_and(|v| v.as_bytes() == b"*")
        && current.is_some()
    {
        return xml_error(StatusCode::PRECONDITION_FAILED, "PreconditionFailed");
    }
    if let Some(expected) = headers.get("if-match") {
        let expected = expected.to_str().unwrap().trim_matches('"').to_owned();
        if current.as_deref().map(|e| e.trim_matches('"')) != Some(expected.as_str()) {
            return xml_error(StatusCode::PRECONDITION_FAILED, "PreconditionFailed");
        }
    }
    bucket.version += 1;
    let etag = format!("\"v{}\"", bucket.version);
    bucket.objects.insert(key, (body.to_vec(), etag.clone()));
    (StatusCode::OK, [(header::ETAG, etag)]).into_response()
}

async fn get_object(
    State(bucket): State<Shared>,
    Path((_bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let mut bucket = bucket.lock().unwrap();
    bucket.seen.push(format!("GET {key}"));
    let Some((body, etag)) = bucket.objects.get(&key).cloned() else {
        return xml_error(StatusCode::NOT_FOUND, "NoSuchKey");
    };
    let Some(range) = headers.get(header::RANGE) else {
        return ([(header::ETAG, etag)], body).into_response();
    };
    let range = range
        .to_str()
        .unwrap()
        .trim_start_matches("bytes=")
        .to_owned();
    let (from, to) = match range.split_once('-') {
        Some(("", suffix)) => {
            let n: usize = suffix.parse().unwrap();
            (body.len().saturating_sub(n), body.len())
        }
        Some((start, "")) => (start.parse().unwrap(), body.len()),
        Some((start, end)) => (
            start.parse().unwrap(),
            (end.parse::<usize>().unwrap() + 1).min(body.len()),
        ),
        None => unreachable!(),
    };
    if from >= body.len() && !body.is_empty() {
        return xml_error(StatusCode::RANGE_NOT_SATISFIABLE, "InvalidRange");
    }
    let part = body[from.min(body.len())..to].to_vec();
    (
        StatusCode::PARTIAL_CONTENT,
        [
            (header::ETAG, etag),
            (
                header::CONTENT_RANGE,
                format!("bytes {from}-{}/{}", to.saturating_sub(1), body.len()),
            ),
        ],
        part,
    )
        .into_response()
}

async fn delete_object(
    State(bucket): State<Shared>,
    Path((_bucket, key)): Path<(String, String)>,
) -> Response {
    let mut bucket = bucket.lock().unwrap();
    bucket.seen.push(format!("DELETE {key}"));
    bucket.objects.remove(&key);
    StatusCode::NO_CONTENT.into_response()
}

async fn list_objects(
    State(bucket): State<Shared>,
    Path(name): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let bucket = bucket.lock().unwrap();
    assert_eq!(query.get("list-type").map(String::as_str), Some("2"));
    let prefix = query.get("prefix").cloned().unwrap_or_default();
    let mut contents = String::new();
    let mut count = 0;
    for (key, (body, etag)) in bucket
        .objects
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix))
    {
        count += 1;
        contents.push_str(&format!(
            "<Contents><Key>{key}</Key><LastModified>2026-10-14T12:00:00.000Z</LastModified><ETag>{}</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
            etag.replace('"', "&quot;"),
            body.len()
        ));
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Name>{name}</Name><Prefix>{prefix}</Prefix><KeyCount>{count}</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>{contents}</ListBucketResult>"
    );
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/xml"),
        )],
        xml,
    )
        .into_response()
}

async fn serve() -> (SocketAddr, Shared) {
    let bucket: Shared = Arc::default();
    let app = Router::new()
        .route("/{bucket}", get(list_objects))
        .route("/{bucket}/", get(list_objects))
        .route(
            "/{bucket}/{*key}",
            get(get_object).put(put_object).delete(delete_object),
        )
        .with_state(bucket.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, bucket)
}

async fn client(addr: SocketAddr) -> S3Store {
    S3Store::connect(&S3Settings {
        bucket: "taba".into(),
        region: Some("us-east-1".into()),
        endpoint: Some(format!("http://{addr}")),
        access_key: Some("key".into()),
        secret_key: Some("secret".into()),
        path_style: true,
    })
    .await
}

/// UC: UC-BKP-003
#[tokio::test]
async fn the_s3_client_does_what_the_object_store_contract_says_against_an_s3_server() {
    let (addr, bucket) = serve().await;
    let store = client(addr).await;

    let etag = store.put("a/1", b"0123456789".to_vec()).await.unwrap();
    assert!(!etag.is_empty());
    assert_eq!(store.get("a/1").await.unwrap().unwrap().body, b"0123456789");
    assert_eq!(store.get_range("a/1", 2, 3).await.unwrap().unwrap(), b"234");
    assert_eq!(
        store.get_range("a/1", 8, 100).await.unwrap().unwrap(),
        b"89"
    );
    assert_eq!(store.get_suffix("a/1", 4).await.unwrap().unwrap(), b"6789");
    assert!(
        store.get("a/2").await.unwrap().is_none(),
        "NoSuchKey becomes None"
    );
    assert!(store.get_range("a/2", 0, 1).await.unwrap().is_none());

    store.put("a/2", b"x".to_vec()).await.unwrap();
    store.put("b/1", b"y".to_vec()).await.unwrap();
    let listed = store.list("a/").await.unwrap();
    assert_eq!(
        listed.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(),
        ["a/1", "a/2"]
    );
    assert_eq!(listed[0].size, 10);
    assert!(!listed[0].etag.is_empty());

    store.delete("a/1").await.unwrap();
    store.delete("a/1").await.unwrap();
    assert!(store.get("a/1").await.unwrap().is_none());
    assert!(
        bucket
            .lock()
            .unwrap()
            .seen
            .iter()
            .any(|s| s == "DELETE a/1")
    );
}

/// UC: UC-BKP-025
#[tokio::test]
async fn conditional_writes_use_if_none_match_and_if_match_and_a_412_is_not_an_error() {
    let (addr, _bucket) = serve().await;
    let store = client(addr).await;

    assert!(
        store
            .put_if_absent("restore/lock.json", b"1".to_vec())
            .await
            .unwrap()
    );
    assert!(
        !store
            .put_if_absent("restore/lock.json", b"2".to_vec())
            .await
            .unwrap(),
        "412: already exists"
    );
    let etag = store.get("restore/lock.json").await.unwrap().unwrap().etag;
    assert!(
        !store
            .put_if_match("restore/lock.json", b"3".to_vec(), "\"other\"")
            .await
            .unwrap()
    );
    assert!(
        store
            .put_if_match("restore/lock.json", b"3".to_vec(), &etag)
            .await
            .unwrap()
    );
    assert_eq!(
        store.get("restore/lock.json").await.unwrap().unwrap().body,
        b"3"
    );
}

/// UC: UC-BKP-004
#[tokio::test]
async fn the_base_prefix_is_a_folder_in_the_bucket() {
    let (addr, bucket) = serve().await;
    let store = Prefixed::new(Arc::new(client(addr).await), "taba/prod");
    store.put("nodes/n1.json", b"{}".to_vec()).await.unwrap();
    assert!(
        bucket
            .lock()
            .unwrap()
            .objects
            .contains_key("taba/prod/nodes/n1.json")
    );
    let listed = store.list("nodes/").await.unwrap();
    assert_eq!(listed[0].key, "nodes/n1.json");
}

/// UC: UC-BKP-001
#[tokio::test]
async fn an_unreachable_s3_is_an_error_the_caller_can_swallow_not_a_panic() {
    let store = S3Store::connect(&S3Settings {
        bucket: "taba".into(),
        region: Some("us-east-1".into()),
        // Closed port: no one listens.
        endpoint: Some("http://127.0.0.1:1".into()),
        access_key: Some("a".into()),
        secret_key: Some("b".into()),
        path_style: true,
    })
    .await;
    assert!(store.put("x", vec![1]).await.is_err());
    assert!(store.list("").await.is_err());
}
