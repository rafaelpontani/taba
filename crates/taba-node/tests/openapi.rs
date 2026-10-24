//! The OpenAPI contract (RF-22, doc 15): what each port serves, and the check that the answers a node really gives
//! are the ones the documents describe (the status is documented and the body matches its schema).

mod common;

use common::{TestNode, TestNodeBuilder, error_code};
use jsonschema::Validator;
use reqwest::Method;
use serde_json::{Value, json};
use taba_node::http::openapi::{Api, document};

const ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";

/// Which port a call goes to.
#[derive(Clone, Copy)]
enum Port {
    Client,
    Operations,
}

fn validator(spec: &Value, schema: &Value) -> Validator {
    // A schema of the document refers to the others by `#/components/schemas/...`: put them next to it.
    let mut root = schema.clone();
    root["components"] = spec["components"].clone();
    jsonschema::validator_for(&root).expect("a valid schema")
}

/// The status is documented for the operation, and a JSON body matches the schema documented for that status.
fn check(spec: &Value, method: &Method, template: &str, status: u16, body: &Value) {
    let operation = &spec["paths"][template][method.as_str().to_lowercase()];
    assert!(
        operation.is_object(),
        "{method} {template} is not in the document"
    );
    let response = &operation["responses"][status.to_string()];
    assert!(
        response.is_object(),
        "{method} {template} answered {status}, which the document does not list: {:?}",
        operation["responses"]
            .as_object()
            .map(|r| r.keys().collect::<Vec<_>>())
    );
    let Some(schema) = response["content"]["application/json"].get("schema") else {
        return;
    };
    let errors: Vec<String> = validator(spec, schema)
        .iter_errors(body)
        .map(|e| format!("{e} (at {})", e.instance_path))
        .collect();
    assert!(
        errors.is_empty(),
        "{method} {template} {status}: the body does not match its schema\n{errors:#?}\nbody: {body}"
    );
}

struct Caller<'a> {
    node: &'a TestNode,
    spec: &'a Value,
    token: Option<&'a str>,
}

impl Caller<'_> {
    /// Calls `path` (a real path) of the operation `template`, expects `status` and checks the answer against the
    /// document. Returns the JSON body (`null` if there is none or it is not JSON).
    async fn call(
        &self,
        port: Port,
        method: Method,
        template: &str,
        path: &str,
        body: Option<(&str, Vec<u8>)>,
        status: u16,
    ) -> Value {
        let url = match port {
            Port::Client => self.node.client_url(path),
            Port::Operations => self.node.operations_url(path),
        };
        let mut request = self.node.client.request(method.clone(), url);
        if let Some(token) = self.token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if let Some((content_type, bytes)) = body {
            request = request.header("content-type", content_type).body(bytes);
        }
        let response = request.send().await.expect("request");
        let got = response.status().as_u16();
        let is_json = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("json"));
        let text = response.text().await.expect("body");
        assert_eq!(got, status, "{method} {path}: {text}");
        let value = if is_json {
            serde_json::from_str(&text).expect("JSON body")
        } else {
            Value::Null
        };
        check(self.spec, &method, template, status, &value);
        value
    }

    async fn get(&self, template: &str, path: &str, status: u16) -> Value {
        self.call(Port::Client, Method::GET, template, path, None, status)
            .await
    }

    async fn send_json(
        &self,
        method: Method,
        template: &str,
        path: &str,
        body: Value,
        status: u16,
    ) -> Value {
        let bytes = serde_json::to_vec(&body).unwrap();
        self.call(
            Port::Client,
            method,
            template,
            path,
            Some(("application/json", bytes)),
            status,
        )
        .await
    }
}

async fn served(node: &TestNode, port: Port, path: &str) -> Value {
    let url = match port {
        Port::Client => node.client_url(path),
        Port::Operations => node.operations_url(path),
    };
    let response = node.client.get(url).send().await.unwrap();
    assert_eq!(response.status(), 200, "{path}");
    response.json().await.unwrap()
}

/// Production (authentication on) with the root token.
fn with_auth() -> TestNodeBuilder {
    TestNode::builder()
        .production()
        .env("TABA_AUTH_ROOT_TOKEN", ROOT)
}

/// UC: UC-CNV-018
#[tokio::test]
async fn each_port_serves_its_own_document_and_an_interface_without_a_token() {
    let node = with_auth().start().await;

    // Authentication is on: the API asks for a token, the documentation does not.
    assert_eq!(node.client_get("/topics").await.status(), 401);
    let client = served(&node, Port::Client, "/openapi/client.json").await;
    assert!(client["openapi"].as_str().unwrap().starts_with("3.1"));
    assert_eq!(client["info"]["title"], "Taba client API");
    assert!(client["paths"]["/topics"].is_object());
    assert_eq!(client["security"], json!([{ "bearer": [] }]));
    assert_eq!(
        client["components"]["securitySchemes"]["bearer"]["scheme"],
        "bearer"
    );

    let monitoring = served(&node, Port::Operations, "/openapi/monitoring.json").await;
    assert_eq!(monitoring["info"]["title"], "Taba monitoring API");
    assert!(monitoring["paths"]["/health/cluster"].is_object());
    assert!(
        monitoring.get("security").is_none(),
        "the monitoring port has no authentication"
    );

    // The interface is embedded in the binary: the page and its assets come from the node itself.
    for (port, name) in [(Port::Client, "client"), (Port::Operations, "operations")] {
        let url = match port {
            Port::Client => node.client_url("/docs/"),
            Port::Operations => node.operations_url("/docs/"),
        };
        let page = node.client.get(url).send().await.unwrap();
        assert_eq!(page.status(), 200, "{name} port");
        assert!(
            page.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html"),
            "{name} port"
        );
        let html = page.text().await.unwrap().to_lowercase();
        assert!(html.contains("swagger"), "{name} port: {html}");
        assert!(
            !html.contains("cdn.") && !html.contains("unpkg.com"),
            "{name} port: the page must not load anything from the internet"
        );
    }

    // Each port only has its own document.
    assert_eq!(
        error_code(node.client_get("/openapi/monitoring.json").await, 404).await,
        "route.not-found"
    );
    let other = node
        .client
        .get(node.operations_url("/openapi/client.json"))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(other, 404).await, "route.not-found");

    // What `taba openapi` prints is what a node with the defaults of production serves.
    let printed: Value = serde_json::from_str(&document(Api::Client)).unwrap();
    assert_eq!(client, printed);
    let printed: Value = serde_json::from_str(&document(Api::Monitoring)).unwrap();
    assert_eq!(monitoring, printed);
    node.shutdown().await;
}

/// UC: UC-CNV-018
#[tokio::test]
async fn the_document_only_lists_the_routes_the_node_has() {
    // Authentication off: there are no `/tokens` routes.
    let node = TestNode::single().await;
    let client = served(&node, Port::Client, "/openapi/client.json").await;
    assert!(client["paths"]["/topics"].is_object());
    assert!(client["paths"].get("/tokens").is_none());
    assert!(client["paths"]["/metrics"].is_object());
    node.shutdown().await;

    // Prometheus off: there is no `/metrics` on either port.
    let node = TestNode::builder()
        .env("TABA_TELEMETRY_PROMETHEUS_ENABLED", "false")
        .start()
        .await;
    let client = served(&node, Port::Client, "/openapi/client.json").await;
    let monitoring = served(&node, Port::Operations, "/openapi/monitoring.json").await;
    assert!(client["paths"].get("/metrics").is_none());
    assert!(monitoring["paths"].get("/metrics").is_none());
    assert!(monitoring["paths"]["/health/live"].is_object());
    node.shutdown().await;
}

/// UC: UC-CNV-018
#[test]
fn every_operation_is_identified_described_and_has_its_security_decided() {
    let client: Value = serde_json::from_str(&document(Api::Client)).unwrap();
    let monitoring: Value = serde_json::from_str(&document(Api::Monitoring)).unwrap();
    let mut ids = std::collections::BTreeSet::new();
    for (name, spec) in [("client", &client), ("monitoring", &monitoring)] {
        for (path, item) in spec["paths"].as_object().unwrap() {
            for (method, operation) in item.as_object().unwrap() {
                let at = format!("{name} {method} {path}");
                let id = operation["operationId"].as_str().unwrap_or_default();
                assert!(!id.is_empty(), "{at} has no operationId");
                assert!(
                    ids.insert(format!("{name}:{id}")),
                    "{at}: repeated operationId"
                );
                assert!(operation["tags"][0].is_string(), "{at} has no tag");
                assert!(
                    operation["responses"]
                        .as_object()
                        .is_some_and(|r| !r.is_empty()),
                    "{at} has no responses"
                );
                // Every JSON error answer is the `ErrorResponse`, but the probes (their own `HealthResponse`).
                for (status, response) in operation["responses"].as_object().unwrap() {
                    if status.starts_with('4') || status.starts_with('5') {
                        let schema = &response["content"]["application/json"]["schema"];
                        let is_health = path.starts_with("/health/");
                        let reference = schema["$ref"].as_str().unwrap_or_default();
                        assert!(
                            reference.ends_with("/ErrorResponse")
                                || (is_health && reference.ends_with("/HealthResponse")),
                            "{at} {status}: {schema}"
                        );
                    }
                }
            }
        }
    }
    // The probes need no token even where everything else does; the metrics take one on the client port only.
    assert_eq!(
        client["paths"]["/health/live"]["get"]["security"],
        json!([{}])
    );
    assert_eq!(
        client["paths"]["/health/ready"]["get"]["security"],
        json!([{}])
    );
    assert!(client["paths"]["/metrics"]["get"].get("security").is_none());
    assert!(monitoring.get("security").is_none());
}

/// UC: UC-CNV-019
#[tokio::test]
async fn the_answers_of_the_client_api_match_the_document() {
    let node = TestNode::single().await;
    let spec = served(&node, Port::Client, "/openapi/client.json").await;
    let api = Caller {
        node: &node,
        spec: &spec,
        token: None,
    };

    // Topics.
    let created = api
        .send_json(
            Method::POST,
            "/topics",
            "/topics",
            json!({ "name": "orders", "type": "queue", "tags": { "team": "a" } }),
            201,
        )
        .await;
    assert_eq!(created["name"], "orders");
    api.send_json(
        Method::POST,
        "/topics",
        "/topics",
        json!({ "name": "hub", "type": "distributor", "prefixes": ["orders"] }),
        201,
    )
    .await;
    api.send_json(
        Method::POST,
        "/topics",
        "/topics",
        json!({ "name": "bad name", "type": "queue" }),
        400,
    )
    .await;
    api.send_json(
        Method::POST,
        "/topics",
        "/topics",
        json!({ "name": "orders", "type": "queue" }),
        409,
    )
    .await;
    let page = api.get("/topics", "/topics?limit=10", 200).await;
    assert!(page["items"].as_array().unwrap().len() >= 3);
    api.get("/topics/{topic}", "/topics/orders", 200).await;
    let dead_letter = api
        .get("/topics/{topic}", "/topics/orders-dead-letter", 200)
        .await;
    assert_eq!(dead_letter["deadLetter"], true);
    api.get("/topics/{topic}", "/topics/nothing", 404).await;
    api.send_json(
        Method::PATCH,
        "/topics/{topic}",
        "/topics/orders",
        json!({ "tags": { "team": "b" } }),
        200,
    )
    .await;
    api.send_json(
        Method::PATCH,
        "/topics/{topic}",
        "/topics/orders",
        json!({ "name": "orders" }),
        400,
    )
    .await;

    // Messages: JSON, text and binary bodies, and a distributor.
    let published = api
        .call(
            Port::Client,
            Method::POST,
            "/topics/{topic}/messages",
            "/topics/orders/messages",
            Some(("application/json", br#"{"n":1}"#.to_vec())),
            201,
        )
        .await;
    assert!(published["id"].is_string());
    api.call(
        Port::Client,
        Method::POST,
        "/topics/{topic}/messages",
        "/topics/orders/messages",
        Some(("text/plain", b"hello".to_vec())),
        201,
    )
    .await;
    api.call(
        Port::Client,
        Method::POST,
        "/topics/{topic}/messages",
        "/topics/orders/messages",
        Some(("image/png", vec![0x89, b'P', b'N', b'G'])),
        201,
    )
    .await;
    let fan_out = api
        .call(
            Port::Client,
            Method::POST,
            "/topics/{topic}/messages",
            "/topics/hub/messages",
            Some(("application/json", br#"{"n":2}"#.to_vec())),
            200,
        )
        .await;
    assert_eq!(fan_out["matched"], 1);
    api.call(
        Port::Client,
        Method::POST,
        "/topics/{topic}/messages",
        "/topics/nothing/messages",
        Some(("application/json", b"1".to_vec())),
        404,
    )
    .await;

    let consumed = api
        .get(
            "/topics/{topic}/messages",
            "/topics/orders/messages?maxMessages=10",
            200,
        )
        .await;
    let messages = consumed["messages"].as_array().unwrap();
    assert!(messages.len() >= 4, "{consumed}");
    assert!(messages.iter().any(|m| m["bodyEncoding"] == "base64"));
    assert!(messages.iter().any(|m| m["body"] == "hello"));
    let first = &messages[0];
    let (id, receipt) = (
        first["id"].as_str().unwrap(),
        first["receiptToken"].as_str().unwrap(),
    );
    api.send_json(
        Method::PUT,
        "/topics/{topic}/messages/{id}/visibility",
        &format!("/topics/orders/messages/{id}/visibility?receiptToken={receipt}"),
        json!({ "visibilityTimeout": 30 }),
        200,
    )
    .await;
    api.call(
        Port::Client,
        Method::DELETE,
        "/topics/{topic}/messages/{id}",
        &format!("/topics/orders/messages/{id}?receiptToken={receipt}"),
        None,
        204,
    )
    .await;
    api.call(
        Port::Client,
        Method::DELETE,
        "/topics/{topic}/messages/{id}",
        &format!("/topics/orders/messages/{id}"),
        None,
        400,
    )
    .await;
    let pruned = api
        .call(
            Port::Client,
            Method::DELETE,
            "/topics/{topic}/messages",
            "/topics/orders/messages",
            None,
            200,
        )
        .await;
    assert!(pruned["removed"].is_u64());

    // Who the caller is, the nodes, the cluster and the statistics.
    let me = api.get("/auth/me", "/auth/me", 200).await;
    assert_eq!(me["root"], true);
    let nodes = api.get("/nodes", "/nodes", 200).await;
    let node_id = nodes["items"][0]["nodeId"].as_str().unwrap().to_owned();
    api.get("/cluster/status", "/cluster/status", 200).await;
    api.get("/cluster/status", "/cluster/status?scope=other", 400)
        .await;
    api.get(
        "/nodes/{nodeId}/drain",
        &format!("/nodes/{node_id}/drain"),
        200,
    )
    .await;
    api.get("/nodes/{nodeId}/drain", "/nodes/nobody/drain", 404)
        .await;
    api.get("/stats/topics", "/stats/topics", 200).await;
    api.get("/stats/topics/{topic}", "/stats/topics/orders", 200)
        .await;
    api.get(
        "/stats/topics/{topic}/groups",
        "/stats/topics/orders/groups",
        200,
    )
    .await;
    api.get("/stats/topics/{topic}", "/stats/topics/nothing", 404)
        .await;

    // The probes, on the client port too.
    api.get("/health/live", "/health/live", 200).await;
    api.get("/health/ready", "/health/ready", 200).await;
    let metrics = node
        .client
        .get(node.client_url("/metrics"))
        .send()
        .await
        .unwrap();
    assert_eq!(metrics.status(), 200);
    check(&spec, &Method::GET, "/metrics", 200, &Value::Null);

    api.call(
        Port::Client,
        Method::DELETE,
        "/topics/{topic}",
        "/topics/hub",
        None,
        204,
    )
    .await;
    node.shutdown().await;
}

/// UC: UC-CNV-019
#[tokio::test]
async fn the_answers_of_the_tokens_routes_match_the_document() {
    let node = with_auth().start().await;
    let spec = served(&node, Port::Client, "/openapi/client.json").await;
    let root = Caller {
        node: &node,
        spec: &spec,
        token: Some(ROOT),
    };
    let anonymous = Caller {
        node: &node,
        spec: &spec,
        token: None,
    };
    anonymous.get("/topics", "/topics", 401).await;

    node.clock.advance(std::time::Duration::from_secs(1));
    let created = root
        .send_json(
            Method::POST,
            "/tokens",
            "/tokens",
            json!({ "name": "app", "permissions": ["publish", "consume"] }),
            201,
        )
        .await;
    let id = created["id"].as_str().unwrap().to_owned();
    let secret = created["token"].as_str().unwrap().to_owned();
    root.send_json(
        Method::POST,
        "/tokens",
        "/tokens",
        json!({ "name": "x", "permissions": ["nothing"] }),
        400,
    )
    .await;
    node.clock.advance(std::time::Duration::from_secs(1));
    let page = root.get("/tokens", "/tokens", 200).await;
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t.get("token").is_none())
    );
    root.get("/tokens/{id}", &format!("/tokens/{id}"), 200)
        .await;
    root.get("/tokens/{id}", "/tokens/nothing", 404).await;
    node.clock.advance(std::time::Duration::from_secs(1));
    root.send_json(
        Method::PATCH,
        "/tokens/{id}",
        &format!("/tokens/{id}"),
        json!({ "name": "app2" }),
        200,
    )
    .await;

    // A plain token: who it is, and what it is refused.
    let plain = Caller {
        node: &node,
        spec: &spec,
        token: Some(&secret),
    };
    let me = plain.get("/auth/me", "/auth/me", 200).await;
    assert_eq!(me["root"], false);
    let refused = plain.get("/tokens", "/tokens", 403).await;
    assert_eq!(refused["required"], "admin");
    plain.get("/cluster/status", "/cluster/status", 403).await;

    node.clock.advance(std::time::Duration::from_secs(1));
    root.call(
        Port::Client,
        Method::DELETE,
        "/tokens/{id}",
        &format!("/tokens/{id}"),
        None,
        204,
    )
    .await;
    node.shutdown().await;
}

/// UC: UC-CNV-019
#[tokio::test]
async fn the_answers_of_the_monitoring_port_match_the_document() {
    let node = TestNode::single().await;
    let client_spec = served(&node, Port::Client, "/openapi/client.json").await;
    let spec = served(&node, Port::Operations, "/openapi/monitoring.json").await;
    let api = Caller {
        node: &node,
        spec: &client_spec,
        token: None,
    };
    api.send_json(
        Method::POST,
        "/topics",
        "/topics",
        json!({ "name": "orders", "type": "queue", "tags": { "team": "a" } }),
        201,
    )
    .await;
    api.call(
        Port::Client,
        Method::POST,
        "/topics/{topic}/messages",
        "/topics/orders/messages",
        Some(("application/json", br#"{"n":1}"#.to_vec())),
        201,
    )
    .await;

    let ops = Caller {
        node: &node,
        spec: &spec,
        token: None,
    };
    for (template, path, status) in [
        ("/health/live", "/health/live", 200),
        ("/health/ready", "/health/ready", 200),
        ("/health/cluster", "/health/cluster", 200),
        ("/health/cluster", "/health/cluster?scope=node", 200),
        ("/health/cluster", "/health/cluster?scope=other", 400),
        ("/metrics/topics", "/metrics/topics", 200),
        ("/metrics/topics", "/metrics/topics?groupBy=team", 200),
        ("/metrics/topics/{topic}", "/metrics/topics/orders", 200),
        ("/metrics/topics/{topic}", "/metrics/topics/nothing", 404),
        (
            "/metrics/topics/{topic}/groups",
            "/metrics/topics/orders/groups",
            200,
        ),
    ] {
        ops.call(Port::Operations, Method::GET, template, path, None, status)
            .await;
    }
    let metrics = node
        .client
        .get(node.operations_url("/metrics"))
        .send()
        .await
        .unwrap();
    assert_eq!(metrics.status(), 200);
    check(&spec, &Method::GET, "/metrics", 200, &Value::Null);
    node.shutdown().await;
}

/// UC: UC-CNV-019
#[test]
#[should_panic(expected = "does not match its schema")]
fn the_check_refuses_a_body_that_does_not_match_its_schema() {
    let spec: Value = serde_json::from_str(&document(Api::Client)).unwrap();
    // A `Topic` has an `id` that is a text and a `name`.
    check(
        &spec,
        &Method::GET,
        "/topics/{topic}",
        200,
        &json!({ "id": 1 }),
    );
}

/// UC: UC-CNV-019
#[test]
#[should_panic(expected = "which the document does not list")]
fn the_check_refuses_a_status_the_document_does_not_list() {
    let spec: Value = serde_json::from_str(&document(Api::Client)).unwrap();
    check(&spec, &Method::GET, "/topics/{topic}", 418, &Value::Null);
}

/// UC: UC-CNV-020
#[test]
fn the_published_documents_are_the_ones_the_server_generates() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../openapi");
    for (api, file) in [
        (Api::Client, "client.json"),
        (Api::Monitoring, "monitoring.json"),
    ] {
        let path = root.join(file);
        let published = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("openapi/{file} is missing ({e}): generate it with `taba openapi`")
        });
        let mut published: Value = serde_json::from_str(&published).expect("a JSON document");
        let mut generated: Value = serde_json::from_str(&document(api)).unwrap();
        // The version is the crate's, which the release pipeline sets to the tag's: it is not what drifts.
        published["info"]["version"] = Value::Null;
        generated["info"]["version"] = Value::Null;
        assert_eq!(
            published,
            generated,
            "openapi/{file} is out of date: run `cargo run -q -- openapi {} > openapi/{file}`",
            file.trim_end_matches(".json")
        );
    }
}

/// UC: UC-CNV-018
#[tokio::test]
async fn the_documentation_of_the_monitoring_port_follows_its_ip_filter() {
    let node = TestNode::builder()
        .env("TABA_OPERATIONS_ALLOWED_IPS", "10.0.0.0/8")
        .start()
        .await;
    // The port is for the collectors inside the infrastructure: from outside the list, nothing on it answers, the
    // document and the interface included. The client port stays open.
    for path in ["/openapi/monitoring.json", "/docs/"] {
        let response = node
            .client
            .get(node.operations_url(path))
            .send()
            .await
            .unwrap();
        assert_eq!(
            error_code(response, 403).await,
            "ops.ip-not-allowed",
            "{path}"
        );
    }
    let client = served(&node, Port::Client, "/openapi/client.json").await;
    assert!(client["paths"]["/topics"].is_object());
    node.shutdown().await;
}
