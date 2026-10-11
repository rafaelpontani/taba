use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Default)]
struct Mock {
    replies: Mutex<VecDeque<Result<Response, TransportError>>>,
    seen: Mutex<Vec<(Request, Timeouts)>>,
}

impl Transport for Arc<Mock> {
    fn send(
        &self,
        _: &Url,
        request: &Request,
        timeouts: &Timeouts,
    ) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push((request.clone(), *timeouts));
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("a chamada não tinha resposta combinada")
    }
}

fn response(status: u16, headers: &[(&str, &str)], body: &str) -> Result<Response, TransportError> {
    Ok(Response {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: body.as_bytes().to_vec(),
    })
}

fn error_body(code: &str) -> String {
    format!("{{\"code\":\"{code}\",\"message\":\"m\",\"requestId\":\"r1\"}}")
}

struct Rig {
    mock: Arc<Mock>,
    sleeps: Arc<Mutex<Vec<Duration>>>,
    client: Client,
}

fn rig(config: Config, replies: Vec<Result<Response, TransportError>>) -> Rig {
    let mock = Arc::new(Mock {
        replies: Mutex::new(replies.into()),
        ..Mock::default()
    });
    let sleeps = Arc::new(Mutex::new(Vec::new()));
    let recorded = sleeps.clone();
    let client = Client::with_transport(
        config,
        Box::new(mock.clone()),
        Arc::new(move |d| recorded.lock().unwrap().push(d)),
    )
    .unwrap();
    Rig {
        mock,
        sleeps,
        client,
    }
}

fn simple(replies: Vec<Result<Response, TransportError>>) -> Rig {
    rig(Config::new("http://taba:8080"), replies)
}

impl Rig {
    fn request(&self, n: usize) -> Request {
        self.mock.seen.lock().unwrap()[n].0.clone()
    }
    fn calls(&self) -> usize {
        self.mock.seen.lock().unwrap().len()
    }
    fn sleeps(&self) -> Vec<u64> {
        self.sleeps
            .lock()
            .unwrap()
            .iter()
            .map(Duration::as_secs)
            .collect()
    }
}

fn header<'a>(r: &'a Request, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

const TOPIC: &str = "{\"id\":\"i1\",\"name\":\"pedidos\",\"type\":\"queue\",\"tags\":{},\"createdAt\":\"2026-10-06T12:00:00.000Z\",\"fifo\":true,\"volatile\":false,\"ttl\":null,\"future\":[1]}";

/// UC: UC-CNT-002
#[test]
fn config_defaults_and_required_base_url() {
    let c = Config::new("http://h");
    assert_eq!(c.connect_timeout, Duration::from_secs(5));
    assert_eq!(c.request_timeout, Duration::from_secs(30));
    assert_eq!(c.max_attempts, 3);
    assert!(c.user_agent.starts_with("taba-rust/"));
    assert!(c.token.is_none());
    let err = Client::new(Config::new("")).err().unwrap();
    assert_eq!(
        (err.status, err.code.as_str()),
        (0, "client.invalid-config")
    );
}

/// UC: UC-CNT-003, UC-CNT-018
#[test]
fn headers_carry_the_token_the_agent_and_the_optional_ids() {
    let mut config = Config::new("http://taba:8080");
    config.token = Some("tb_abc".into());
    let rig = rig(config, vec![response(200, &[], TOPIC)]);
    let mut call = Call::new("GET", "/topics/pedidos");
    call.request_id = Some("req-1".into());
    call.traceparent = Some("00-aa-bb-01".into());
    rig.client.call(&call).unwrap();
    let r = rig.request(0);
    assert_eq!(header(&r, "Authorization"), Some("Bearer tb_abc"));
    assert_eq!(header(&r, "Accept"), Some("application/json"));
    assert!(header(&r, "User-Agent").unwrap().starts_with("taba-rust/"));
    assert_eq!(header(&r, "X-Request-Id"), Some("req-1"));
    assert_eq!(header(&r, "traceparent"), Some("00-aa-bb-01"));
    assert_eq!(header(&r, "Content-Type"), None);
}

/// UC: UC-CNT-003
#[test]
fn without_a_token_no_call_carries_authorization() {
    let rig = simple(vec![response(200, &[], TOPIC)]);
    rig.client.get_topic("pedidos").unwrap();
    assert_eq!(header(&rig.request(0), "Authorization"), None);
}

/// UC: UC-CNT-001, UC-CNT-029
#[test]
fn topic_operations_build_the_expected_routes() {
    let rig = simple(vec![
        response(201, &[], TOPIC),
        response(200, &[], TOPIC),
        response(
            200,
            &[],
            &format!("{{\"items\":[{TOPIC}],\"nextCursor\":\"c2\"}}"),
        ),
        response(200, &[], TOPIC),
        response(204, &[], ""),
    ]);
    let c = &rig.client;
    let created = c
        .create_topic(&TopicRequest {
            name: "pedidos".into(),
            topic_type: "queue".into(),
            fifo: Some(true),
            tags: vec![("time".into(), "fin".into())],
            ttl: Some(86400),
            ..TopicRequest::default()
        })
        .unwrap();
    assert_eq!(
        (created.name.as_str(), created.fifo, created.volatile),
        ("pedidos", Some(true), Some(false))
    );
    c.get_topic("pedidos").unwrap();
    let page = c
        .list_topics(Some(2), Some("cur sor"), Some("ped"))
        .unwrap();
    assert_eq!(
        (page.items.len(), page.next_cursor.as_deref()),
        (1, Some("c2"))
    );
    c.update_topic(
        "pedidos",
        &TopicUpdate {
            ttl: Nullable::Default,
            ..TopicUpdate::default()
        },
    )
    .unwrap();
    c.delete_topic("pedidos").unwrap();

    let t: Vec<(String, String)> = (0..5)
        .map(|n| {
            let r = rig.request(n);
            (r.method, r.target)
        })
        .collect();
    assert_eq!(t[0], ("POST".into(), "/topics".into()));
    assert_eq!(t[1], ("GET".into(), "/topics/pedidos".into()));
    assert_eq!(
        t[2],
        (
            "GET".into(),
            "/topics?limit=2&cursor=cur%20sor&name=ped".into()
        )
    );
    assert_eq!(t[3], ("PATCH".into(), "/topics/pedidos".into()));
    assert_eq!(t[4], ("DELETE".into(), "/topics/pedidos".into()));
    assert_eq!(
        String::from_utf8(rig.request(0).body).unwrap(),
        "{\"name\":\"pedidos\",\"type\":\"queue\",\"tags\":{\"time\":\"fin\"},\"fifo\":true,\"ttl\":86400}"
    );
    assert_eq!(
        String::from_utf8(rig.request(3).body).unwrap(),
        "{\"ttl\":null}"
    );
    assert_eq!(
        header(&rig.request(0), "Content-Type"),
        Some("application/json")
    );
    assert!(rig
        .request(3)
        .headers
        .iter()
        .all(|(k, _)| !k.eq_ignore_ascii_case("X-HTTP-Method-Override")));
}

/// UC: UC-CNT-007
#[test]
fn list_all_topics_follows_next_cursor_until_the_last_page() {
    let rig = simple(vec![
        response(
            200,
            &[],
            &format!("{{\"items\":[{TOPIC}],\"nextCursor\":\"p2\"}}"),
        ),
        response(200, &[], &format!("{{\"items\":[{TOPIC},{TOPIC}]}}")),
    ]);
    assert_eq!(rig.client.list_all_topics(None).unwrap().len(), 3);
    assert_eq!(rig.request(1).target, "/topics?cursor=p2");
}

/// UC: UC-CNT-004
#[test]
fn publish_sends_raw_bytes_with_content_type_and_query() {
    let rig = simple(vec![
        response(201, &[], "{\"id\":\"m1\",\"duplicate\":false}"),
        response(200, &[], "{\"matched\":2,\"deliveries\":[]}"),
        response(201, &[], "{\"id\":\"m2\"}"),
    ]);
    let options = PublishOptions {
        content_type: Some("image/png".into()),
        group_id: Some("g 1".into()),
        deduplication_id: Some("d1".into()),
        request_id: Some("rq".into()),
        fifo: true,
    };
    let bytes = [0x89, b'P', 0, 255];
    let r = rig.client.publish("arquivos", &bytes, &options).unwrap();
    assert_eq!((r.id.as_deref(), r.duplicate), (Some("m1"), Some(false)));
    let req = rig.request(0);
    assert_eq!(
        req.target,
        "/topics/arquivos/messages?groupId=g%201&deduplicationId=d1"
    );
    assert_eq!(req.body, bytes);
    assert_eq!(header(&req, "Content-Type"), Some("image/png"));
    assert_eq!(header(&req, "X-Request-Id"), Some("rq"));

    let d = rig.client.publish_json("dist", "{\"a\":1}").unwrap();
    assert_eq!((d.id, d.matched), (None, Some(2)));
    assert_eq!(
        header(&rig.request(1), "Content-Type"),
        Some("application/json")
    );
    rig.client.publish_text("t", "olá").unwrap();
    assert_eq!(
        header(&rig.request(2), "Content-Type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(rig.request(2).body, "olá".as_bytes());
}

/// UC: UC-CNT-005, UC-CNT-008
#[test]
fn consume_sends_only_what_was_given_and_reads_for_wait_time_plus_ten() {
    let rig = simple(vec![
        response(200, &[], "{\"messages\":[]}"),
        response(200, &[], "{\"messages\":[]}"),
    ]);
    assert!(rig
        .client
        .consume("fila", &ConsumeOptions::default())
        .unwrap()
        .is_empty());
    assert_eq!(rig.request(0).target, "/topics/fila/messages");
    assert_eq!(
        rig.mock.seen.lock().unwrap()[0].1.total,
        Duration::from_secs(10)
    );

    let options = ConsumeOptions {
        max_messages: Some(5),
        visibility_timeout: Some(60),
        wait_time: Some(20),
        auto_create: true,
        volatile: Some(false),
        idle_timeout: Some(900),
        fifo: Some(true),
        tags: vec![("time".into(), "fin".into())],
        ..ConsumeOptions::default()
    };
    rig.client.consume("fila", &options).unwrap();
    assert_eq!(
        rig.request(1).target,
        "/topics/fila/messages?maxMessages=5&visibilityTimeout=60&waitTime=20&autoCreate=true&volatile=false&idleTimeout=900&fifo=true&tag.time=fin"
    );
    // 20 s de espera com `requestTimeout` de 30 s não derruba a chamada: o limite é 30 s.
    assert_eq!(
        rig.mock.seen.lock().unwrap()[1].1.total,
        Duration::from_secs(30)
    );
}

/// UC: UC-CNT-005
#[test]
fn auto_create_parameters_are_left_out_without_auto_create() {
    let rig = simple(vec![response(200, &[], "{\"messages\":[]}")]);
    let options = ConsumeOptions {
        volatile: Some(false),
        fifo: Some(true),
        ..ConsumeOptions::default()
    };
    rig.client.consume("fila", &options).unwrap();
    assert_eq!(rig.request(0).target, "/topics/fila/messages");
}

/// UC: UC-CNT-006
#[test]
fn ack_and_visibility_put_the_receipt_token_in_the_query() {
    let rig = simple(vec![
        response(204, &[], ""),
        response(
            200,
            &[],
            "{\"id\":\"m1\",\"invisibleUntil\":\"2026-10-06T12:02:30.000Z\"}",
        ),
    ]);
    rig.client.ack("fila", "m1", "rt 1").unwrap();
    let v = rig
        .client
        .change_visibility("fila", "m1", "rt1", 0)
        .unwrap();
    assert_eq!(v.invisible_until, "2026-10-06T12:02:30.000Z");
    let (a, b) = (rig.request(0), rig.request(1));
    assert_eq!(
        (a.method.as_str(), a.target.as_str()),
        ("DELETE", "/topics/fila/messages/m1?receiptToken=rt%201")
    );
    assert_eq!(
        (b.method.as_str(), b.target.as_str()),
        (
            "PUT",
            "/topics/fila/messages/m1/visibility?receiptToken=rt1"
        )
    );
    assert_eq!(
        String::from_utf8(b.body).unwrap(),
        "{\"visibilityTimeout\":0}"
    );
}

/// UC: UC-CNT-015, UC-CNT-016, UC-CNT-026
#[test]
fn messages_come_back_as_bytes_for_json_text_and_base64() {
    let body = "{\"messages\":[\
        {\"id\":\"a\",\"body\":{ \"pedido\": 9001 },\"contentType\":\"application/json\",\"groupId\":\"g\",\"deduplicationId\":\"d\",\"receiptToken\":\"r1\",\"deliveryCount\":2,\"publishedAt\":\"p\",\"invisibleUntil\":\"i\",\"novo\":true},\
        {\"id\":\"b\",\"body\":\"Pedido \\u00e9 ok\",\"contentType\":\"text/plain; charset=utf-8\",\"receiptToken\":\"r2\",\"deliveryCount\":1,\"publishedAt\":\"p\",\"invisibleUntil\":\"i\"},\
        {\"id\":\"c\",\"body\":\"iVBORw0KGgo=\",\"contentType\":\"image/png\",\"bodyEncoding\":\"base64\",\"receiptToken\":\"r3\",\"deliveryCount\":1,\"publishedAt\":\"p\",\"invisibleUntil\":\"i\",\"deadLetteredAt\":\"dl\",\"deadLetterReason\":\"max-deliveries\"},\
        {\"id\":\"d\",\"body\":[1,2],\"contentType\":\"application/vnd.x+json\",\"receiptToken\":\"r4\",\"deliveryCount\":1,\"publishedAt\":\"p\",\"invisibleUntil\":\"i\"}]}";
    let rig = simple(vec![response(200, &[], body)]);
    let m = rig
        .client
        .consume("fila", &ConsumeOptions::default())
        .unwrap();
    assert_eq!(m.len(), 4);
    assert_eq!(m[0].body, b"{ \"pedido\": 9001 }");
    assert_eq!(
        (
            m[0].group_id.as_deref(),
            m[0].deduplication_id.as_deref(),
            m[0].delivery_count
        ),
        (Some("g"), Some("d"), 2)
    );
    assert_eq!(m[1].body, "Pedido é ok".as_bytes());
    assert_eq!(m[2].body, [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    assert_eq!(
        (
            m[2].dead_lettered_at.as_deref(),
            m[2].dead_letter_reason.as_deref()
        ),
        (Some("dl"), Some("max-deliveries"))
    );
    assert_eq!(m[3].body, b"[1,2]");
    assert_eq!(m[0].receipt_token, "r1");
    assert!(m[0].raw.starts_with("{\"id\":\"a\""));
}

/// UC: UC-CNT-009, UC-CNT-017
#[test]
fn a_timeout_after_sending_is_not_repeated() {
    let rig = simple(vec![Err(TransportError::Timeout { sent: true })]);
    let err = rig
        .client
        .consume("fila", &ConsumeOptions::default())
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (0, "client.timeout"));
    assert!(err.is_client_side());
    assert_eq!(rig.calls(), 1);
}

/// UC: UC-CNT-010
#[test]
fn retries_429_and_503_waiting_for_retry_after_then_backoff() {
    let rig = simple(vec![
        response(
            429,
            &[("Retry-After", "7")],
            &error_body("rate-limit.exceeded"),
        ),
        response(503, &[], &error_body("cluster.quorum-unavailable")),
        response(200, &[], TOPIC),
    ]);
    rig.client.get_topic("pedidos").unwrap();
    assert_eq!(rig.calls(), 3);
    assert_eq!(rig.sleeps(), [7, 2]);
}

/// UC: UC-CNT-010
#[test]
fn backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts() {
    let mut config = Config::new("http://taba");
    config.max_attempts = 4;
    let rig = rig(
        config,
        (0..4)
            .map(|_| response(503, &[], &error_body("cluster.node-syncing")))
            .collect(),
    );
    let err = rig.client.get_topic("x").unwrap_err();
    assert_eq!(err.status, 503);
    assert_eq!(rig.calls(), 4);
    assert_eq!(rig.sleeps(), [1, 2, 4]);
}

/// UC: UC-CNT-011
#[test]
fn after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success() {
    let rig = simple(vec![
        response(
            503,
            &[("Retry-After", "1")],
            &error_body("cluster.commit-timeout"),
        ),
        response(409, &[], &error_body("topic.already-exists")),
        response(200, &[], TOPIC),
        response(503, &[], &error_body("cluster.commit-timeout")),
        response(404, &[], &error_body("topic.not-found")),
    ]);
    let t = rig
        .client
        .create_topic(&TopicRequest {
            name: "pedidos".into(),
            topic_type: "queue".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(t.name, "pedidos");
    assert_eq!(rig.request(2).method, "GET");
    rig.client.delete_topic("pedidos").unwrap();
}

/// UC: UC-CNT-011, UC-CNT-014
#[test]
fn without_a_previous_503_a_409_or_404_is_a_plain_error() {
    let rig = simple(vec![
        response(409, &[], &error_body("topic.already-exists")),
        response(404, &[], &error_body("topic.not-found")),
    ]);
    let req = TopicRequest {
        name: "p".into(),
        topic_type: "queue".into(),
        ..Default::default()
    };
    assert_eq!(
        rig.client.create_topic(&req).unwrap_err().code,
        "topic.already-exists"
    );
    assert_eq!(
        rig.client.delete_topic("p").unwrap_err().code,
        "topic.not-found"
    );
    assert_eq!(rig.calls(), 2);
}

/// UC: UC-CNT-012
#[test]
fn gateway_errors_are_only_repeated_when_publishing_to_a_fifo() {
    let gateway = || response(502, &[], &error_body("forward.bad-gateway"));
    let rig = simple(vec![gateway()]);
    assert_eq!(
        rig.client
            .publish("fila", b"{}", &PublishOptions::default())
            .unwrap_err()
            .status,
        502
    );
    assert_eq!(rig.calls(), 1);

    let rig = simple(vec![
        gateway(),
        response(504, &[], &error_body("forward.timeout")),
        response(201, &[], "{\"id\":\"m\",\"duplicate\":true}"),
    ]);
    let fifo = PublishOptions {
        fifo: true,
        group_id: Some("g".into()),
        ..Default::default()
    };
    let r = rig.client.publish("fila", b"{}", &fifo).unwrap();
    assert_eq!(r.duplicate, Some(true));
    assert_eq!(rig.calls(), 3);
}

/// UC: UC-CNT-013
#[test]
fn network_failure_before_sending_is_repeated_and_after_is_not() {
    let rig = simple(vec![
        Err(TransportError::Connect("recusada".into())),
        Err(TransportError::Timeout { sent: false }),
        response(200, &[], TOPIC),
    ]);
    rig.client.get_topic("x").unwrap();
    assert_eq!(rig.calls(), 3);
    assert_eq!(rig.sleeps(), [1, 2]);

    let rig = simple(vec![Err(TransportError::Io("reset".into()))]);
    let err = rig
        .client
        .publish("f", b"{}", &PublishOptions::default())
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (0, "client.network"));
    assert_eq!(rig.calls(), 1);

    let rig = simple(
        (0..3)
            .map(|_| Err(TransportError::Connect("recusada".into())))
            .collect(),
    );
    assert_eq!(
        rig.client.get_topic("x").unwrap_err().code,
        "client.network"
    );
    assert_eq!(rig.calls(), 3);
}

/// UC: UC-CNT-014
#[test]
fn a_4xx_other_than_429_is_never_repeated() {
    for status in [400, 401, 403, 404, 409, 413, 422] {
        let rig = simple(vec![response(status, &[], &error_body("x.y"))]);
        assert_eq!(rig.client.get_topic("x").unwrap_err().status, status);
        assert_eq!(rig.calls(), 1, "{status}");
    }
}

/// UC: UC-CNT-017
#[test]
fn the_error_carries_the_error_response_fields() {
    let body = "{\"code\":\"validation.failed\",\"message\":\"A requisição tem campos inválidos.\",\"requestId\":\"abc\",\"details\":[{\"field\":\"name\",\"code\":\"validation.required\",\"message\":\"x\"}],\"novo\":1}";
    let rig = simple(vec![response(400, &[("X-Request-Id", "hdr")], body)]);
    let err = rig.client.get_topic("x").unwrap_err();
    assert_eq!(err.status, 400);
    assert_eq!(err.code, "validation.failed");
    assert_eq!(err.message, "A requisição tem campos inválidos.");
    assert_eq!(err.request_id.as_deref(), Some("abc"));
    assert!(err
        .details
        .as_deref()
        .unwrap()
        .starts_with("[{\"field\":\"name\""));
    assert_eq!(err.body, body.as_bytes());
    assert_eq!(
        err.to_string(),
        "validation.failed: A requisição tem campos inválidos. (requestId abc)"
    );
}

/// UC: UC-CNT-017, UC-CNT-018
#[test]
fn retry_after_and_request_id_come_from_the_headers_and_a_non_json_error_still_reports() {
    let mut config = Config::new("http://t");
    config.max_attempts = 1;
    let rig = rig(
        config,
        vec![response(
            503,
            &[("Retry-After", "5"), ("X-Request-Id", "hdr")],
            "upstream sad",
        )],
    );
    let err = rig.client.get_topic("x").unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.message.as_str()),
        ("http.503", "upstream sad")
    );
    assert_eq!(
        (err.retry_after, err.request_id.as_deref()),
        (Some(5), Some("hdr"))
    );
}

/// UC: UC-CNT-018
#[test]
fn the_reply_exposes_the_request_id_header() {
    let rig = simple(vec![response(200, &[("x-request-id", "r9")], TOPIC)]);
    let reply = rig.client.call(&Call::new("GET", "/topics/x")).unwrap();
    assert_eq!(reply.request_id.as_deref(), Some("r9"));
}

/// UC: UC-CNT-031
#[test]
fn subscribe_acks_only_what_the_handler_accepted_and_stops_on_demand() {
    let msg = |id: &str| {
        format!("{{\"id\":\"{id}\",\"body\":\"x\",\"contentType\":\"text/plain\",\"receiptToken\":\"rt-{id}\",\"deliveryCount\":1,\"publishedAt\":\"p\",\"invisibleUntil\":\"i\"}}")
    };
    let rig = simple(vec![
        response(200, &[], &format!("{{\"messages\":[{}]}}", msg("1"))),
        response(204, &[], ""),
        response(200, &[], &format!("{{\"messages\":[{}]}}", msg("2"))),
        response(200, &[], &format!("{{\"messages\":[{}]}}", msg("3"))),
        response(204, &[], ""),
    ]);
    let mut seen = Vec::new();
    let handled = std::cell::Cell::new(0);
    rig.client
        .subscribe(
            "fila",
            &ConsumeOptions::default(),
            || handled.get() >= 3,
            |m| {
                seen.push(m.id.clone());
                handled.set(handled.get() + 1);
                if m.id == "2" {
                    Err("falhou".into())
                } else {
                    Ok(())
                }
            },
        )
        .ok();
    assert_eq!(seen, ["1", "2", "3"]);
    let targets: Vec<String> = (0..rig.calls()).map(|n| rig.request(n).target).collect();
    assert_eq!(
        targets[0],
        "/topics/fila/messages?maxMessages=1&waitTime=20"
    );
    assert!(targets.contains(&"/topics/fila/messages/1?receiptToken=rt-1".to_owned()));
    assert!(
        !targets.iter().any(|t| t.contains("/messages/2?")),
        "a 2 não deve ser confirmada: {targets:?}"
    );
    assert!(targets.contains(&"/topics/fila/messages/3?receiptToken=rt-3".to_owned()));
}

/// UC: UC-CNT-028
#[test]
fn unknown_fields_in_responses_are_ignored() {
    let t = Topic::parse(TOPIC).unwrap();
    assert_eq!(t.id, "i1");
    assert_eq!(CONTRACT_VERSION, "1");
}

fn assert_send_sync<T: Send + Sync>() {}

/// UC: UC-CNT-020
#[test]
fn the_client_is_safe_to_share_between_threads() {
    assert_send_sync::<Client>();
    assert_send_sync::<Config>();
    assert_send_sync::<Error>();
}

/// UC: UC-CNT-020
#[test]
fn many_threads_can_use_one_client_at_the_same_time() {
    use crate::testing::{client, MockTransport};
    let mock = MockTransport::new();
    for _ in 0..16 {
        mock.reply(200, &[], "{\"id\":\"i\",\"name\":\"n\",\"type\":\"queue\"}");
    }
    let shared = Arc::new(client(Config::new("http://taba:8080"), &mock));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let c = shared.clone();
            std::thread::spawn(move || c.get_topic("n").map(|t| t.name))
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap().unwrap(), "n");
    }
    assert_eq!(mock.seen().len(), 16);
}

/// UC: UC-CNT-023
#[test]
fn the_only_dependencies_are_rustls_and_its_root_certificates() {
    let manifest = include_str!("../../Cargo.toml");
    let deps: Vec<&str> = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap()
        .lines()
        .take_while(|l| !l.starts_with('['))
        .filter_map(|l| l.split('=').next())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    assert_eq!(deps, ["rustls", "rustls-native-certs"]);
    assert!(!manifest.contains("[dev-dependencies]"));
}
