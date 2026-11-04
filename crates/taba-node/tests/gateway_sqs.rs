//! The SQS gateway on a real node (doc 39; level **I**): the port, the two protocols, the signature, the permissions of the
//! token, the drain, and the same queue reached by the native API. The AWS SDKs themselves are tested by
//! `connectors/gateway-contract/`; here the requests are written by hand, so a failure points at the node.

mod common;

use std::net::SocketAddr;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{eventually, eventually_async, wait_joined};
use taba_node::core::clock::Clock;
use taba_node::gateway::aws::sigv4::{self, Signed};

const DEV_SECRET: &str = "dev-secret-with-at-least-32-characters";
const REGION: &str = "eu-west-1";

fn builder() -> common::TestNodeBuilder {
    TestNode::builder()
        .env("TABA_GATEWAY_SQS_ENABLED", "true")
        .env("TABA_GATEWAY_SQS_PORT", "0")
}

fn gateway(node: &TestNode) -> SocketAddr {
    node.node.gateway_sqs_addr().expect("the SQS gateway is on")
}

/// A call of the JSON protocol, not signed (for a node with the authentication off).
async fn json_call(node: &TestNode, action: &str, body: Value) -> (u16, Value) {
    let response = node
        .client
        .post(format!("http://{}/", gateway(node)))
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

/// A call of the Query protocol, not signed.
async fn query_call(node: &TestNode, form: &str) -> (u16, String) {
    let response = node
        .client
        .post(format!("http://{}/", gateway(node)))
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

/// A call of the JSON protocol signed with SigV4 as an SDK would.
async fn signed_call(
    node: &TestNode,
    access_key: &str,
    secret: &str,
    action: &str,
    body: Value,
) -> (u16, Value) {
    let addr = gateway(node);
    let body = body.to_string();
    let now = time::OffsetDateTime::from(node.clock.now());
    let amz_date = now
        .format(time::macros::format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .expect("date");
    let host = addr.to_string();
    let target = format!("AmazonSQS.{action}");
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("host", host.parse().unwrap());
    headers.insert("x-amz-date", amz_date.parse().unwrap());
    headers.insert("x-amz-target", target.parse().unwrap());
    headers.insert(
        "content-type",
        "application/x-amz-json-1.0".parse().unwrap(),
    );
    let authorization = sigv4::sign(
        access_key,
        secret,
        REGION,
        "sqs",
        &amz_date,
        &Signed {
            method: "POST",
            path: "/",
            query: "",
            headers: &headers,
            body: body.as_bytes(),
        },
        &["content-type", "host", "x-amz-date", "x-amz-target"],
    );
    headers.insert("authorization", authorization.parse().unwrap());
    let response = node
        .client
        .post(format!("http://{addr}/"))
        .headers(headers)
        .body(body)
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

fn error_type(body: &Value) -> &str {
    body["__type"]
        .as_str()
        .unwrap_or("")
        .rsplit('#')
        .next()
        .unwrap_or("")
}

/// UC: UC-GTW-017
#[tokio::test]
async fn with_the_key_off_the_gateway_does_not_open_a_port() {
    let node = TestNode::single().await;
    assert!(node.node.gateway_sqs_addr().is_none());
    node.shutdown().await;
}

/// UC: UC-GTW-017
#[tokio::test]
async fn without_authentication_both_protocols_share_the_queues_and_a_native_client_reads_them() {
    let node = builder().start().await;
    wait_joined(&node).await;

    // JSON creates and sends, Query receives: the same queue.
    let (status, created) =
        json_call(&node, "CreateQueue", json!({"QueueName": "gw-shared"})).await;
    assert_eq!(status, 200, "{created}");
    let url = created["QueueUrl"].as_str().unwrap().to_owned();
    assert!(url.ends_with("/000000000000/gw-shared"), "{url}");

    let (status, sent) = json_call(
        &node,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "from sqs"}),
    )
    .await;
    assert_eq!(status, 200, "{sent}");

    let (status, xml) = query_call(
        &node,
        &format!(
            "Action=ReceiveMessage&QueueUrl={}&MaxNumberOfMessages=1&VisibilityTimeout=30&WaitTimeSeconds=0&AttributeName.1=All",
            url.replace(':', "%3A").replace('/', "%2F")
        ),
    )
    .await;
    assert_eq!(status, 200, "{xml}");
    assert!(xml.contains("<Body>from sqs</Body>"), "{xml}");
    assert!(
        xml.contains("<Name>ApproximateReceiveCount</Name><Value>1</Value>"),
        "{xml}"
    );

    // The native API sees the same queue; what the gateway sent is plain text, and what a native client sends is read by SQS.
    let native = node
        .client
        .post(node.client_url("/topics/gw-shared/messages"))
        .header("content-type", "text/plain")
        .body("from native")
        .send()
        .await
        .unwrap();
    assert_eq!(native.status(), 201);
    let (_, received) = json_call(
        &node,
        "ReceiveMessage",
        json!({"QueueUrl": url, "MaxNumberOfMessages": 10, "VisibilityTimeout": 30, "WaitTimeSeconds": 2}),
    )
    .await;
    assert_eq!(received["Messages"][0]["Body"], "from native");
    node.shutdown().await;
}

/// UC: UC-GTW-017
#[tokio::test]
async fn a_message_with_attributes_is_an_envelope_for_a_native_consumer_and_plain_text_without_them()
 {
    let node = builder().start().await;
    wait_joined(&node).await;
    let (_, created) = json_call(&node, "CreateQueue", json!({"QueueName": "gw-envelope"})).await;
    let url = created["QueueUrl"].as_str().unwrap().to_owned();

    json_call(
        &node,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "plain"}),
    )
    .await;
    json_call(
        &node,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "with attributes",
            "MessageAttributes": {"color": {"DataType": "String", "StringValue": "blue"}}}),
    )
    .await;

    let native = node
        .client_get("/topics/gw-envelope/messages?maxMessages=10&visibilityTimeout=30")
        .await;
    let messages = native.json::<Value>().await.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(messages.len(), 2);
    let plain = messages
        .iter()
        .find(|m| m["body"] == "plain")
        .expect("plain text");
    assert_eq!(plain["contentType"], "text/plain; charset=utf-8");
    let enveloped = messages
        .iter()
        .find(|m| m["body"].is_object())
        .expect("envelope");
    assert_eq!(enveloped["body"]["tabaSqs"], 1);
    assert_eq!(enveloped["body"]["body"], "with attributes");
    assert_eq!(enveloped["body"]["attributes"]["color"]["value"], "blue");
    node.shutdown().await;
}

/// UC: UC-GTW-017
#[tokio::test]
async fn the_attributes_of_a_queue_come_back_and_the_counts_follow_the_messages() {
    let node = builder().start().await;
    wait_joined(&node).await;
    let (_, created) = json_call(
        &node,
        "CreateQueue",
        json!({"QueueName": "gw-attrs", "Attributes": {"VisibilityTimeout": "45", "MessageRetentionPeriod": "86400"}}),
    )
    .await;
    let url = created["QueueUrl"].as_str().unwrap().to_owned();
    for body in ["a", "b", "c"] {
        json_call(
            &node,
            "SendMessage",
            json!({"QueueUrl": url, "MessageBody": body}),
        )
        .await;
    }
    json_call(
        &node,
        "ReceiveMessage",
        json!({"QueueUrl": url, "MaxNumberOfMessages": 1, "VisibilityTimeout": 60}),
    )
    .await;
    let (status, attrs) = json_call(
        &node,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["All"]}),
    )
    .await;
    assert_eq!(status, 200, "{attrs}");
    let a = &attrs["Attributes"];
    assert_eq!(a["VisibilityTimeout"], "45");
    assert_eq!(a["MessageRetentionPeriod"], "86400");
    assert_eq!(a["QueueArn"], "arn:aws:sqs:us-east-1:000000000000:gw-attrs");
    assert_eq!(a["ApproximateNumberOfMessages"], "2");
    assert_eq!(a["ApproximateNumberOfMessagesNotVisible"], "1");
    assert_eq!(a["ApproximateNumberOfMessagesDelayed"], "0");
    node.shutdown().await;
}

/// UC: UC-GTW-018
#[tokio::test]
async fn with_authentication_the_key_decides_what_the_gateway_may_do() {
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
        let eventual = || async {
            node.client
                .post(node.client_url("/tokens"))
                .header("authorization", bearer)
                .json(&body)
                .send()
                .await
                .unwrap()
        };
        let mut response = eventual().await;
        for _ in 0..100 {
            if response.status() == 201 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            response = eventual().await;
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
    let mut created = (0, Value::Null);
    for _ in 0..100 {
        created = signed_call(
            &node,
            "KEYALL",
            "secret-all",
            "CreateQueue",
            json!({"QueueName": "signed-q"}),
        )
        .await;
        if created.0 == 200 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(created.0, 200, "{}", created.1);
    let url = created.1["QueueUrl"].as_str().unwrap().to_owned();

    // The permissions of the token apply: consume only cannot create or send, and a prefix confines the queues.
    let (status, denied) = signed_call(
        &node,
        "KEYCONSUME",
        "secret-consume",
        "CreateQueue",
        json!({"QueueName": "other-q"}),
    )
    .await;
    assert_eq!(
        (status, error_type(&denied)),
        (403, "AccessDenied"),
        "{denied}"
    );
    let (status, denied) = signed_call(
        &node,
        "KEYCONSUME",
        "secret-consume",
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "x"}),
    )
    .await;
    assert_eq!(
        (status, error_type(&denied)),
        (403, "AccessDenied"),
        "{denied}"
    );
    let (status, mine) = signed_call(
        &node,
        "KEYPREFIX",
        "secret-prefix",
        "CreateQueue",
        json!({"QueueName": "mine-q"}),
    )
    .await;
    assert_eq!(status, 200, "{mine}");
    let (status, denied) = signed_call(
        &node,
        "KEYPREFIX",
        "secret-prefix",
        "CreateQueue",
        json!({"QueueName": "theirs-q"}),
    )
    .await;
    assert_eq!(
        (status, error_type(&denied)),
        (403, "AccessDenied"),
        "{denied}"
    );

    // The signature: a wrong secret, an unknown key, and no signature at all.
    let (status, bad) = signed_call(&node, "KEYALL", "wrong-secret", "ListQueues", json!({})).await;
    assert_eq!(
        (status, error_type(&bad)),
        (403, "SignatureDoesNotMatch"),
        "{bad}"
    );
    let (status, unknown) = signed_call(&node, "KEYNONE", "secret", "ListQueues", json!({})).await;
    assert_eq!(
        (status, error_type(&unknown)),
        (403, "InvalidClientTokenId"),
        "{unknown}"
    );
    let (status, none) = json_call(&node, "ListQueues", json!({})).await;
    assert_eq!(
        (status, error_type(&none)),
        (403, "MissingAuthenticationToken"),
        "{none}"
    );

    // A queue list only shows what the token reaches.
    let (_, listed) =
        signed_call(&node, "KEYPREFIX", "secret-prefix", "ListQueues", json!({})).await;
    let urls = listed["QueueUrls"].as_array().cloned().unwrap_or_default();
    assert!(
        urls.iter().all(|u| u.as_str().unwrap().contains("mine-")),
        "{listed}"
    );
    node.shutdown().await;
}

/// UC: UC-GTW-019
#[tokio::test]
async fn a_draining_node_tells_the_sdk_to_try_again() {
    let node = builder().start().await;
    wait_joined(&node).await;
    let (_, created) = json_call(&node, "CreateQueue", json!({"QueueName": "gw-drain"})).await;
    let url = created["QueueUrl"].as_str().unwrap().to_owned();

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
        .post(format!("http://{}/", gateway(&node)))
        .header("x-amz-target", "AmazonSQS.SendMessage")
        .body(json!({"QueueUrl": url, "MessageBody": "x"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert!(response.headers().contains_key("retry-after"));
    assert_eq!(
        response.headers()["x-amzn-query-error"],
        "ServiceUnavailable;Receiver",
        "a receiver fault, so the SDK retries"
    );
    node.shutdown().await;
}

/// UC: UC-GTW-024
#[tokio::test]
async fn a_fifo_queue_is_the_fifo_topic_of_the_name_without_the_suffix() {
    let node = builder().start().await;
    wait_joined(&node).await;

    let (status, created) = json_call(
        &node,
        "CreateQueue",
        json!({"QueueName": "gw-orders.fifo", "Attributes": {"FifoQueue": "true"}}),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let url = created["QueueUrl"].as_str().unwrap().to_owned();
    assert!(url.ends_with("/000000000000/gw-orders.fifo"), "{url}");

    // The native API sees a FIFO topic without the suffix.
    let topic = node.client_get("/topics/gw-orders").await;
    assert_eq!(topic.status(), 200);
    assert_eq!(topic.json::<Value>().await.unwrap()["fifo"], true);

    // The kind comes from the name: the same topic is not a standard queue, and the name cannot be taken twice.
    let (status, wrong) = json_call(&node, "GetQueueUrl", json!({"QueueName": "gw-orders"})).await;
    assert_eq!(status, 400, "{wrong}");
    assert_eq!(error_type(&wrong), "QueueDoesNotExist");
    let (status, taken) = json_call(&node, "CreateQueue", json!({"QueueName": "gw-orders"})).await;
    assert_eq!(status, 400, "{taken}");
    assert_eq!(error_type(&taken), "QueueNameExists");
    let (_, attributes) = json_call(
        &node,
        "GetQueueAttributes",
        json!({"QueueUrl": url, "AttributeNames": ["All"]}),
    )
    .await;
    assert_eq!(attributes["Attributes"]["FifoQueue"], "true");
    assert_eq!(
        attributes["Attributes"]["ContentBasedDeduplication"],
        "false"
    );
    assert!(
        attributes["Attributes"]["QueueArn"]
            .as_str()
            .unwrap()
            .ends_with(":gw-orders.fifo")
    );
    let (_, listed) = json_call(&node, "ListQueues", json!({"QueueNamePrefix": "gw-orders"})).await;
    assert_eq!(listed["QueueUrls"], json!([url]));

    // A group is delivered in order and one message at a time; a repeated deduplication id is one message.
    let send = |body: &str, dedup: &str| json!({"QueueUrl": url, "MessageBody": body, "MessageGroupId": "g1", "MessageDeduplicationId": dedup});
    let (_, first) = json_call(&node, "SendMessage", send("one", "d1")).await;
    assert!(first["SequenceNumber"].as_str().unwrap().len() > 10);
    let (_, again) = json_call(&node, "SendMessage", send("one", "d1")).await;
    assert_eq!(
        again["MessageId"], first["MessageId"],
        "a duplicate is the same message"
    );
    json_call(&node, "SendMessage", send("two", "d2")).await;
    let receive = json!({"QueueUrl": url, "MaxNumberOfMessages": 10, "VisibilityTimeout": 30,
        "WaitTimeSeconds": 0, "AttributeNames": ["All"]});
    let (_, got) = json_call(&node, "ReceiveMessage", receive.clone()).await;
    let messages = got["Messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "one message of the group at a time");
    assert_eq!(messages[0]["Body"], "one");
    assert_eq!(messages[0]["Attributes"]["MessageGroupId"], "g1");
    assert_eq!(messages[0]["Attributes"]["MessageDeduplicationId"], "d1");
    let (_, none) = json_call(&node, "ReceiveMessage", receive.clone()).await;
    assert!(none["Messages"].is_null(), "the next waits for the delete");
    json_call(
        &node,
        "DeleteMessage",
        json!({"QueueUrl": url, "ReceiptHandle": messages[0]["ReceiptHandle"]}),
    )
    .await;
    let (_, next) = json_call(&node, "ReceiveMessage", receive).await;
    assert_eq!(next["Messages"][0]["Body"], "two");
    node.shutdown().await;
}

/// UC: UC-GTW-024
#[tokio::test]
async fn a_fifo_send_checks_the_group_the_deduplication_and_the_delay_as_sqs_does() {
    let node = builder().start().await;
    wait_joined(&node).await;
    let (_, created) = json_call(
        &node,
        "CreateQueue",
        json!({"QueueName": "gw-strict.fifo", "Attributes": {"FifoQueue": "true"}}),
    )
    .await;
    let url = created["QueueUrl"].as_str().unwrap().to_owned();
    let error = |body: Value| {
        let node = &node;
        async move {
            let (status, answer) = json_call(node, "SendMessage", body).await;
            assert_eq!(status, 400, "{answer}");
            error_type(&answer).to_owned()
        }
    };
    assert_eq!(
        error(json!({"QueueUrl": url, "MessageBody": "x", "MessageDeduplicationId": "d"})).await,
        "MissingParameter"
    );
    assert_eq!(
        error(json!({"QueueUrl": url, "MessageBody": "x", "MessageGroupId": "g"})).await,
        "InvalidParameterValue"
    );
    assert_eq!(
        error(
            json!({"QueueUrl": url, "MessageBody": "x", "MessageGroupId": "g",
            "MessageDeduplicationId": "d", "DelaySeconds": 5})
        )
        .await,
        "InvalidParameterValue"
    );
    // A queue cannot be made FIFO with a delay, which Taba does not have there.
    let (status, delayed) = json_call(
        &node,
        "CreateQueue",
        json!({"QueueName": "gw-delayed.fifo", "Attributes": {"FifoQueue": "true", "DelaySeconds": "5"}}),
    )
    .await;
    assert_eq!(status, 400, "{delayed}");
    assert_eq!(error_type(&delayed), "UnsupportedOperation");

    // Content-based deduplication: the body is the id.
    let (_, cb) = json_call(
        &node,
        "CreateQueue",
        json!({"QueueName": "gw-cb.fifo",
            "Attributes": {"FifoQueue": "true", "ContentBasedDeduplication": "true"}}),
    )
    .await;
    let cb_url = cb["QueueUrl"].as_str().unwrap().to_owned();
    let body = json!({"QueueUrl": cb_url, "MessageBody": "same", "MessageGroupId": "g"});
    let (_, one) = json_call(&node, "SendMessage", body.clone()).await;
    let (_, two) = json_call(&node, "SendMessage", body).await;
    assert_eq!(one["MessageId"], two["MessageId"]);
    node.shutdown().await;
}

/// UC: UC-GTW-020
#[tokio::test]
async fn a_group_id_with_punctuation_taba_does_not_accept_comes_back_as_it_was_sent() {
    let node = builder().start().await;
    wait_joined(&node).await;
    let (_, created) = json_call(
        &node,
        "CreateQueue",
        json!({"QueueName": "gw-punct.fifo", "Attributes": {"FifoQueue": "true"}}),
    )
    .await;
    let url = created["QueueUrl"].as_str().unwrap().to_owned();
    let (status, sent) = json_call(
        &node,
        "SendMessage",
        json!({"QueueUrl": url, "MessageBody": "odd", "MessageGroupId": "tenant/42!blue",
            "MessageDeduplicationId": "order#7"}),
    )
    .await;
    assert_eq!(status, 200, "{sent}");
    let (_, got) = json_call(
        &node,
        "ReceiveMessage",
        json!({"QueueUrl": url, "VisibilityTimeout": 30, "WaitTimeSeconds": 0, "AttributeNames": ["All"]}),
    )
    .await;
    assert_eq!(got["Messages"][0]["Body"], "odd");
    assert_eq!(
        got["Messages"][0]["Attributes"]["MessageGroupId"],
        "tenant/42!blue"
    );
    assert_eq!(
        got["Messages"][0]["Attributes"]["MessageDeduplicationId"],
        "order#7"
    );
    node.shutdown().await;
}

/// UC: UC-GTW-023
#[tokio::test]
async fn the_queue_named_as_the_dead_letter_queue_shows_what_the_source_dead_letters() {
    let node = builder().start().await;
    wait_joined(&node).await;
    let create = |name: &str, policy: Option<&str>| {
        let node = &node;
        let mut attributes = json!({"FifoQueue": "true"});
        if let Some(policy) = policy {
            attributes["RedrivePolicy"] = json!(policy);
        }
        let body = json!({"QueueName": name, "Attributes": attributes});
        async move { json_call(node, "CreateQueue", body).await }
    };
    let (_, made) = create("gw-dlq.fifo", None).await;
    let dlq = made["QueueUrl"].as_str().unwrap().to_owned();
    let (_, attributes) = json_call(
        &node,
        "GetQueueAttributes",
        json!({"QueueUrl": dlq, "AttributeNames": ["QueueArn"]}),
    )
    .await;
    let arn = attributes["Attributes"]["QueueArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let policy = json!({"deadLetterTargetArn": arn, "maxReceiveCount": 1}).to_string();
    let (status, made) = create("gw-source.fifo", Some(&policy)).await;
    assert_eq!(status, 200, "{made}");
    let source = made["QueueUrl"].as_str().unwrap().to_owned();

    // The policy comes back naming the queue the application chose.
    let (_, shown) = json_call(
        &node,
        "GetQueueAttributes",
        json!({"QueueUrl": source, "AttributeNames": ["RedrivePolicy"]}),
    )
    .await;
    assert!(
        shown["Attributes"]["RedrivePolicy"]
            .as_str()
            .unwrap()
            .contains("gw-dlq.fifo"),
        "{shown}"
    );

    // A queue is the dead-letter queue of one source only.
    let (status, second) = create("gw-other.fifo", Some(&policy)).await;
    assert_eq!(status, 400, "{second}");
    assert_eq!(error_type(&second), "UnsupportedOperation");
    // A dead-letter queue that does not exist is refused before the source is created.
    let missing = json!({"deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:nope.fifo",
        "maxReceiveCount": 1})
    .to_string();
    let (status, refused) = create("gw-third.fifo", Some(&missing)).await;
    assert_eq!(status, 400, "{refused}");
    assert_eq!(error_type(&refused), "InvalidAttributeValue");
    let (status, _) = json_call(&node, "GetQueueUrl", json!({"QueueName": "gw-third.fifo"})).await;
    assert_eq!(status, 400, "the source was not created");

    json_call(
        &node,
        "SendMessage",
        json!({"QueueUrl": source, "MessageBody": "poison", "MessageGroupId": "g", "MessageDeduplicationId": "p"}),
    )
    .await;
    let (_, got) = json_call(
        &node,
        "ReceiveMessage",
        json!({"QueueUrl": source, "VisibilityTimeout": 1, "WaitTimeSeconds": 0}),
    )
    .await;
    assert_eq!(got["Messages"][0]["Body"], "poison");
    node.clock.advance(std::time::Duration::from_secs(5));
    eventually_async("the message in the dead-letter queue", || async {
        let (_, got) = json_call(
            &node,
            "ReceiveMessage",
            json!({"QueueUrl": dlq, "VisibilityTimeout": 30, "WaitTimeSeconds": 0}),
        )
        .await;
        got["Messages"][0]["Body"] == "poison"
    })
    .await;

    // Deleting the source frees the dead-letter queue to be the one of another.
    json_call(&node, "DeleteQueue", json!({"QueueUrl": source})).await;
    let (status, reused) = create("gw-other.fifo", Some(&policy)).await;
    assert_eq!(status, 200, "{reused}");
    node.shutdown().await;
}
