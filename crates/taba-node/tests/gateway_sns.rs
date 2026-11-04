//! The SNS gateway on a real node (doc 39; level **I**): the port, the signature, the permissions of the token, the drain, and
//! the copies the distributor makes to the queues of the SQS gateway. The AWS SDKs themselves are tested by
//! `connectors/gateway-contract/`; here the requests are written by hand, so a failure points at the node.

mod common;

use std::net::SocketAddr;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{eventually, wait_joined};
use taba_node::core::clock::Clock;
use taba_node::gateway::aws::sigv4::{self, Signed};

const DEV_SECRET: &str = "dev-secret-with-at-least-32-characters";
const REGION: &str = "eu-west-1";

/// Both gateways on, so a queue made through SQS can be subscribed to a topic made through SNS.
fn builder() -> common::TestNodeBuilder {
    TestNode::builder()
        .env("TABA_GATEWAY_SQS_ENABLED", "true")
        .env("TABA_GATEWAY_SNS_ENABLED", "true")
        .env("TABA_GATEWAY_AWS_PORT", "0")
}

/// SQS and SNS are served on the same port.
fn sns_addr(node: &TestNode) -> SocketAddr {
    node.node.gateway_addr().expect("the gateway is on")
}

fn sqs_addr(node: &TestNode) -> SocketAddr {
    sns_addr(node)
}

/// A call of the SQS JSON protocol, not signed.
async fn sqs(node: &TestNode, action: &str, body: Value) -> (u16, Value) {
    let response = node
        .client
        .post(format!("http://{}/", sqs_addr(node)))
        .header("x-amz-target", format!("AmazonSQS.{action}"))
        .header("content-type", "application/x-amz-json-1.0")
        .body(body.to_string())
        .send()
        .await
        .expect("request");
    let status = response.status().as_u16();
    let text = response.text().await.expect("body");
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

/// A call of the SNS Query protocol, not signed.
async fn sns(node: &TestNode, form: &str) -> (u16, String) {
    let response = node
        .client
        .post(format!("http://{}/", sns_addr(node)))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form.to_owned())
        .send()
        .await
        .expect("request");
    (
        response.status().as_u16(),
        response.text().await.expect("body"),
    )
}

/// A call of the SNS Query protocol signed with SigV4 as an SDK would, for the service `service`.
async fn signed_sns(
    node: &TestNode,
    access_key: &str,
    secret: &str,
    service: &str,
    form: &str,
) -> (u16, String) {
    let addr = sns_addr(node);
    let now = time::OffsetDateTime::from(node.clock.now());
    let amz_date = now
        .format(time::macros::format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .expect("date");
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("host", addr.to_string().parse().unwrap());
    headers.insert("x-amz-date", amz_date.parse().unwrap());
    headers.insert(
        "content-type",
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    let authorization = sigv4::sign(
        access_key,
        secret,
        REGION,
        service,
        &amz_date,
        &Signed {
            method: "POST",
            path: "/",
            query: "",
            headers: &headers,
            body: form.as_bytes(),
        },
        &["content-type", "host", "x-amz-date"],
    );
    headers.insert("authorization", authorization.parse().unwrap());
    let response = node
        .client
        .post(format!("http://{addr}/"))
        .headers(headers)
        .body(form.to_owned())
        .send()
        .await
        .expect("request");
    (
        response.status().as_u16(),
        response.text().await.expect("body"),
    )
}

/// The text of the first element with that name.
fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(&xml[start..end])
}

async fn queue(node: &TestNode, name: &str) -> (String, String) {
    let (status, created) = sqs(node, "CreateQueue", json!({"QueueName": name})).await;
    assert_eq!(status, 200, "{created}");
    let url = created["QueueUrl"].as_str().unwrap().to_owned();
    let (_, attributes) = sqs(
        node,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["QueueArn"]}),
    )
    .await;
    let arn = attributes["Attributes"]["QueueArn"]
        .as_str()
        .unwrap()
        .to_owned();
    (url, arn)
}

async fn receive(node: &TestNode, url: &str) -> Vec<Value> {
    let (_, got) = sqs(
        node,
        "ReceiveMessage",
        json!({"QueueUrl": url, "MaxNumberOfMessages": 10, "VisibilityTimeout": 30, "WaitTimeSeconds": 2}),
    )
    .await;
    got["Messages"].as_array().cloned().unwrap_or_default()
}

/// UC: UC-GTW-030
#[tokio::test]
async fn one_port_serves_both_protocols_and_a_protocol_that_is_off_says_which_key_turns_it_on() {
    let node = TestNode::single().await;
    assert!(node.node.gateway_addr().is_none(), "both off: no port");
    node.shutdown().await;

    let only_sqs = TestNode::builder()
        .env("TABA_GATEWAY_SQS_ENABLED", "true")
        .env("TABA_GATEWAY_AWS_PORT", "0")
        .start()
        .await;
    assert!(only_sqs.node.gateway_addr().is_some());
    let (status, body) = sns(&only_sqs, "Action=ListTopics").await;
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("gateway.sns.enabled"), "{body}");
    only_sqs.shutdown().await;

    let only_sns = TestNode::builder()
        .env("TABA_GATEWAY_SNS_ENABLED", "true")
        .env("TABA_GATEWAY_AWS_PORT", "0")
        .start()
        .await;
    assert!(only_sns.node.gateway_addr().is_some());
    let (status, body) = sqs(&only_sns, "ListQueues", json!({})).await;
    assert_eq!(status, 404, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("gateway.sqs.enabled"),
        "{body}"
    );
    let (status, body) = sns(&only_sns, "Action=ListTopics").await;
    assert_eq!(status, 200, "{body}");
    only_sns.shutdown().await;
}

/// UC: UC-GTW-029
#[tokio::test]
async fn a_publication_reaches_the_queues_subscribed_and_only_those_and_the_native_api_sees_the_same()
 {
    let node = builder().start().await;
    wait_joined(&node).await;
    let (url_a, arn_a) = queue(&node, "gwsns-a").await;
    let (url_b, arn_b) = queue(&node, "gwsns-b").await;

    let (status, created) = sns(&node, "Action=CreateTopic&Name=gwsns-t").await;
    assert_eq!(status, 200, "{created}");
    let topic = element(&created, "TopicArn").unwrap().to_owned();
    assert_eq!(topic, "arn:aws:sns:us-east-1:000000000000:gwsns-t");

    for arn in [&arn_a, &arn_b] {
        let (status, subscribed) = sns(
            &node,
            &format!("Action=Subscribe&TopicArn={topic}&Protocol=sqs&Endpoint={arn}"),
        )
        .await;
        assert_eq!(status, 200, "{subscribed}");
    }

    // The native API sees a distributor with the names of the queues as its prefixes.
    let native = node.client_get("/topics/gwsns-t").await;
    assert_eq!(native.status(), 200);
    let native: Value = native.json().await.unwrap();
    assert_eq!(native["type"], "distributor");
    let mut prefixes: Vec<String> = native["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_owned())
        .collect();
    prefixes.sort();
    assert_eq!(prefixes, vec!["gwsns-a", "gwsns-b"]);

    // One publication, one copy in each queue, in the envelope of SNS with the id the publisher got.
    let (status, published) = sns(
        &node,
        &format!("Action=Publish&TopicArn={topic}&Message=hello&Subject=greeting"),
    )
    .await;
    assert_eq!(status, 200, "{published}");
    let id = element(&published, "MessageId").unwrap().to_owned();
    for url in [&url_a, &url_b] {
        let messages = receive(&node, url).await;
        assert_eq!(messages.len(), 1, "{url}");
        let notification: Value =
            serde_json::from_str(messages[0]["Body"].as_str().unwrap()).unwrap();
        assert_eq!(notification["Type"], "Notification");
        assert_eq!(notification["Message"], "hello");
        assert_eq!(notification["Subject"], "greeting");
        assert_eq!(notification["TopicArn"], topic.as_str());
        assert_eq!(notification["MessageId"], id.as_str());
    }

    // After the unsubscription the queue gets nothing more.
    let (_, listed) = sns(
        &node,
        &format!("Action=ListSubscriptionsByTopic&TopicArn={topic}"),
    )
    .await;
    assert_eq!(listed.matches("<SubscriptionArn>").count(), 2, "{listed}");
    let sub_b = format!("{topic}:gwsns-b");
    let (status, _) = sns(
        &node,
        &format!("Action=Unsubscribe&SubscriptionArn={sub_b}"),
    )
    .await;
    assert_eq!(status, 200);
    sns(
        &node,
        &format!("Action=Publish&TopicArn={topic}&Message=after"),
    )
    .await;
    let for_a = receive(&node, &url_a).await;
    assert_eq!(
        serde_json::from_str::<Value>(for_a.last().unwrap()["Body"].as_str().unwrap()).unwrap()["Message"],
        "after"
    );
    let (_, for_b) = sqs(
        &node,
        "ReceiveMessage",
        json!({"QueueUrl": url_b, "MaxNumberOfMessages": 10, "VisibilityTimeout": 30, "WaitTimeSeconds": 0}),
    )
    .await;
    assert!(for_b["Messages"].is_null(), "unsubscribed: {for_b}");

    // The kinds stay apart: SQS does not see the topic as a queue, and SNS does not see a queue as a topic.
    let (_, queues) = sqs(&node, "ListQueues", json!({"QueueNamePrefix": "gwsns-"})).await;
    let names: Vec<&str> = queues["QueueUrls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u.as_str().unwrap().rsplit('/').next().unwrap())
        .collect();
    assert_eq!(names, vec!["gwsns-a", "gwsns-b"], "{queues}");
    let (status, not_a_queue) = sqs(&node, "GetQueueUrl", json!({"QueueName": "gwsns-t"})).await;
    assert_eq!(status, 400, "{not_a_queue}");
    let (status, not_a_topic) = sns(
        &node,
        "Action=Publish&TopicArn=arn:aws:sns:us-east-1:000000000000:gwsns-a&Message=x",
    )
    .await;
    assert_eq!(status, 404, "{not_a_topic}");
    assert_eq!(element(&not_a_topic, "Code"), Some("NotFound"));
    let (status, topics) = sns(&node, "Action=ListTopics").await;
    assert_eq!(status, 200);
    assert_eq!(topics.matches("<TopicArn>").count(), 1, "{topics}");

    // Deleting the topic leaves the queues.
    let (status, _) = sns(&node, &format!("Action=DeleteTopic&TopicArn={topic}")).await;
    assert_eq!(status, 200);
    assert_eq!(node.client_get("/topics/gwsns-t").await.status(), 404);
    assert_eq!(node.client_get("/topics/gwsns-a").await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-GTW-029
#[tokio::test]
async fn a_publication_from_the_native_api_and_one_from_sns_are_the_same_to_the_queue() {
    let node = builder().start().await;
    wait_joined(&node).await;
    let (url, arn) = queue(&node, "gwsns-native").await;
    let (_, created) = sns(&node, "Action=CreateTopic&Name=gwsns-native-t").await;
    let topic = element(&created, "TopicArn").unwrap().to_owned();
    sns(
        &node,
        &format!("Action=Subscribe&TopicArn={topic}&Protocol=sqs&Endpoint={arn}"),
    )
    .await;

    // A prefix added by the native API is a subscription for SNS, and the other way round.
    let patched = node
        .client
        .patch(node.client_url("/topics/gwsns-native-t"))
        .json(&json!({"prefixes": ["gwsns-native"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(patched.status(), 200);
    let (_, listed) = sns(
        &node,
        &format!("Action=ListSubscriptionsByTopic&TopicArn={topic}"),
    )
    .await;
    assert!(
        listed.contains(&format!("<Endpoint>{arn}</Endpoint>")),
        "{listed}"
    );

    let native = node
        .client
        .post(node.client_url("/topics/gwsns-native-t/messages"))
        .header("content-type", "application/json")
        .body(r#"{"n":1}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(native.status(), 200);
    let messages = receive(&node, &url).await;
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]["Body"], r#"{"n":1}"#,
        "what the native API published is the body, with no envelope"
    );
    node.shutdown().await;
}

/// UC: UC-GTW-029
#[tokio::test]
async fn with_authentication_the_key_and_the_service_of_the_signature_decide_what_sns_may_do() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("aws.json");
    std::fs::write(&file, "{}").unwrap();
    let node = builder()
        .env("TABA_AUTH_DEV_TOKEN", DEV_SECRET)
        .env("TABA_AUTH_RELOAD_INTERVAL", "1s")
        .env("TABA_GATEWAY_AWS_CREDENTIALS_FILE", file.to_str().unwrap())
        .start()
        .await;
    wait_joined(&node).await;
    let bearer = format!("Bearer tb_dev.{DEV_SECRET}");

    async fn token(node: &TestNode, bearer: &str, body: Value) -> String {
        let attempt = || async {
            node.client
                .post(node.client_url("/tokens"))
                .header("authorization", bearer)
                .json(&body)
                .send()
                .await
                .unwrap()
        };
        let mut response = attempt().await;
        for _ in 0..100 {
            if response.status() == 201 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            response = attempt().await;
        }
        assert_eq!(response.status(), 201);
        response.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    let all = token(
        &node,
        &bearer,
        json!({"name": "all", "permissions": ["publish", "consume", "create", "manage"]}),
    )
    .await;
    let consume = token(
        &node,
        &bearer,
        json!({"name": "consume", "permissions": ["consume"]}),
    )
    .await;
    let prefixed = token(
        &node,
        &bearer,
        json!({"name": "prefixed", "permissions": ["publish", "consume", "create", "manage"], "topicPrefixes": ["mine-"]}),
    )
    .await;
    std::fs::write(
        &file,
        json!({
            "KEYALL": {"secret": "secret-all", "token": all},
            "KEYCONSUME": {"secret": "secret-consume", "token": consume},
            "KEYPREFIX": {"secret": "secret-prefix", "token": prefixed},
        })
        .to_string(),
    )
    .unwrap();

    // The file is read again by the node: wait until the first key is known.
    let mut created = (0, String::new());
    for _ in 0..100 {
        created = signed_sns(
            &node,
            "KEYALL",
            "secret-all",
            "sns",
            "Action=CreateTopic&Name=signed-t",
        )
        .await;
        if created.0 == 200 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(created.0, 200, "{}", created.1);
    let topic = element(&created.1, "TopicArn").unwrap().to_owned();
    assert!(
        topic.contains(REGION),
        "the ARN carries the region of the signature: {topic}"
    );

    let code = |body: &str| element(body, "Code").unwrap_or("-").to_owned();
    // The permissions of the token apply: consume only cannot create or publish, a prefix confines the topics.
    let (status, denied) = signed_sns(
        &node,
        "KEYCONSUME",
        "secret-consume",
        "sns",
        "Action=CreateTopic&Name=other-t",
    )
    .await;
    assert_eq!(
        (status, code(&denied)),
        (403, "AuthorizationError".into()),
        "{denied}"
    );
    let (status, denied) = signed_sns(
        &node,
        "KEYCONSUME",
        "secret-consume",
        "sns",
        &format!("Action=Publish&TopicArn={topic}&Message=x"),
    )
    .await;
    assert_eq!(
        (status, code(&denied)),
        (403, "AuthorizationError".into()),
        "{denied}"
    );
    let (status, mine) = signed_sns(
        &node,
        "KEYPREFIX",
        "secret-prefix",
        "sns",
        "Action=CreateTopic&Name=mine-t",
    )
    .await;
    assert_eq!(status, 200, "{mine}");
    let (status, denied) = signed_sns(
        &node,
        "KEYPREFIX",
        "secret-prefix",
        "sns",
        "Action=CreateTopic&Name=theirs-t",
    )
    .await;
    assert_eq!(
        (status, code(&denied)),
        (403, "AuthorizationError".into()),
        "{denied}"
    );

    // The signature: a wrong secret, an unknown key, and no signature at all. A call signed for another service goes to
    // that service (the credential scope decides), which does not know the action.
    let (status, wrong_service) =
        signed_sns(&node, "KEYALL", "secret-all", "sqs", "Action=ListTopics").await;
    assert_eq!(
        (status, code(&wrong_service)),
        (400, "InvalidAction".into()),
        "{wrong_service}"
    );
    let (status, bad) =
        signed_sns(&node, "KEYALL", "wrong-secret", "sns", "Action=ListTopics").await;
    assert_eq!(
        (status, code(&bad)),
        (403, "SignatureDoesNotMatch".into()),
        "{bad}"
    );
    let (status, unknown) =
        signed_sns(&node, "KEYNONE", "secret", "sns", "Action=ListTopics").await;
    assert_eq!(
        (status, code(&unknown)),
        (403, "InvalidClientTokenId".into()),
        "{unknown}"
    );
    let (status, none) = sns(&node, "Action=ListTopics").await;
    assert_eq!(
        (status, code(&none)),
        (403, "MissingAuthenticationToken".into()),
        "{none}"
    );

    // A list only shows what the token reaches.
    let (_, listed) = signed_sns(
        &node,
        "KEYPREFIX",
        "secret-prefix",
        "sns",
        "Action=ListTopics",
    )
    .await;
    assert_eq!(listed.matches("<TopicArn>").count(), 1, "{listed}");
    assert!(listed.contains(":mine-t<"), "{listed}");
    node.shutdown().await;
}

/// UC: UC-GTW-029
#[tokio::test]
async fn a_draining_node_tells_the_sdk_to_try_again() {
    let node = builder().start().await;
    wait_joined(&node).await;
    let (status, _) = sns(&node, "Action=CreateTopic&Name=gwsns-drain").await;
    assert_eq!(status, 200);

    let started = node
        .client
        .post(node.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert!(started.status().is_success());
    eventually("the node to stop serving", || {
        node.node.shared().drain_state() != taba_node::cluster::state::DrainState::Active
    })
    .await;

    let response = node
        .client
        .post(format!("http://{}/", sns_addr(&node)))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("Action=Publish&TopicArn=arn:aws:sns:us-east-1:000000000000:gwsns-drain&Message=x")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert!(response.headers().contains_key("retry-after"));
    let xml = response.text().await.unwrap();
    assert_eq!(element(&xml, "Code"), Some("ServiceUnavailable"), "{xml}");
    assert_eq!(
        element(&xml, "Type"),
        Some("Receiver"),
        "a receiver fault, so the SDK retries"
    );
    node.shutdown().await;
}
