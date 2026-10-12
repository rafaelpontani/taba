//! A exportação OTLP (doc 11, "Como ligar um coletor", "Traces" e "Logs"; doc 20c): o nó empurra
//! métricas, traces e logs para um coletor falso, configurado só pelas variáveis `OTEL_*`.
//!
//! Os spans e os logs saem pelo assinante global do `tracing`, e há um só por processo; por isso os
//! testes deste arquivo andam um de cada vez (`serial`).

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::cluster::TestCluster;
use common::otlp::{Collector, hex, log_attributes, span_attributes};
use common::{TestNode, series};

/// Um de cada vez: o assinante global e o encaixe do OTLP são do processo.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let guard = LOCK.lock().await;
    taba_node::telemetry::init_logging(Some("warn"), taba_node::config::LogFormat::Text);
    guard
}

fn node_with(collector: &Collector, env: &[(&str, &str)]) -> common::TestNodeBuilder {
    let mut builder = TestNode::builder()
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint)
        .env("OTEL_METRIC_EXPORT_INTERVAL", "200")
        .env("OTEL_TRACES_SAMPLER", "always_on")
        .env("TABA_TELEMETRY_LOGS_LEVEL", "debug");
    for (name, value) in env {
        builder = builder.env(name, value);
    }
    builder
}

async fn create_queue(node: &TestNode, body: Value) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{body}");
}

async fn publish(node: &TestNode, topic: &str, query: &str) -> Value {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages{query}")))
        .header("content-type", "text/plain")
        .body("olá")
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    response.json().await.unwrap()
}

/// Um nó com a exportação ligada, uma fila com tag e uma publicação já feita.
async fn busy_node(collector: &Collector, env: &[(&str, &str)]) -> (TestNode, String) {
    let node = node_with(collector, env).start().await;
    create_queue(
        &node,
        json!({"name": "pedidos", "type": "queue", "tags": {"team": "cobranca"}}),
    )
    .await;
    let id = publish(&node, "pedidos", "").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (node, id)
}

/// UC: UC-OPS-039, UC-OPS-013, UC-OPS-040
#[tokio::test(flavor = "multi_thread")]
async fn otlp_over_http_protobuf_sends_metrics_traces_and_logs_with_the_headers() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let (node, _) = busy_node(
        &collector,
        &[
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
            (
                "OTEL_EXPORTER_OTLP_HEADERS",
                "authorization=Bearer abc, x-tenant=t1",
            ),
            (
                "OTEL_RESOURCE_ATTRIBUTES",
                "service.namespace=prod,deployment.environment.name=staging",
            ),
        ],
    )
    .await;
    node.node.flush_otlp().await;
    collector
        .eventually("métricas, traces e logs", |c| {
            !c.metrics().is_empty() && !c.spans().is_empty() && !c.log_records().is_empty()
        })
        .await;

    // Os cabeçalhos do coletor vão em todas as chamadas.
    assert!(!collector.headers().is_empty());
    for headers in collector.headers() {
        assert_eq!(headers["authorization"], "Bearer abc");
        assert_eq!(headers["x-tenant"], "t1");
    }

    // Os atributos de recurso estão nos três sinais (UC-OPS-040).
    for (signal, resources) in collector.resources().iter().enumerate() {
        assert!(!resources.is_empty(), "sinal {signal} sem recurso");
        for resource in resources {
            assert_eq!(resource["service.name"], "taba", "sinal {signal}");
            assert_eq!(resource["service.namespace"], "prod");
            assert_eq!(resource["deployment.environment.name"], "staging");
            assert_eq!(resource["service.instance.id"], node.node.node_id());
            assert_eq!(resource["service.version"], env!("CARGO_PKG_VERSION"));
        }
    }

    // O OTLP lê o mesmo registro de `GET /metrics` (UC-OPS-013): os mesmos nomes, no vocabulário
    // do OpenTelemetry (com pontos) em vez do do Prometheus (com `_`).
    let text = node.metrics().await;
    let metrics = collector.metrics();
    assert!(!metrics.is_empty());
    for metric in &metrics {
        let prefix = metric.name.replace('.', "_");
        assert!(
            text.lines()
                .any(|l| l.starts_with("# TYPE ") && l.contains(&prefix)),
            "`{}` está no OTLP e não em /metrics",
            metric.name
        );
    }
    let names: Vec<&str> = metrics.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"http.server.request.duration"));
    assert!(names.contains(&"taba.node.state"));
    assert!(series(&text, "process_uptime_seconds").is_some());
    node.shutdown().await;
}

/// UC: UC-OPS-039
#[tokio::test(flavor = "multi_thread")]
async fn otlp_defaults_to_grpc_and_sends_the_three_signals() {
    let _serial = serial().await;
    let collector = Collector::grpc(None).await;
    let (node, _) = busy_node(&collector, &[("OTEL_EXPORTER_OTLP_HEADERS", "x-tenant=t1")]).await;
    node.node.flush_otlp().await;
    collector
        .eventually("métricas, traces e logs por gRPC", |c| {
            !c.metrics().is_empty() && !c.spans().is_empty() && !c.log_records().is_empty()
        })
        .await;
    for headers in collector.headers() {
        assert_eq!(headers["x-tenant"], "t1");
    }
    node.shutdown().await;
}

/// UC: UC-OPS-039
#[tokio::test(flavor = "multi_thread")]
async fn otlp_over_grpc_trusts_the_certificate_in_otel_exporter_otlp_certificate() {
    let _serial = serial().await;
    let key = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()]).unwrap();
    let certificate = params.self_signed(&key).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let ca = dir.path().join("coletor.pem");
    std::fs::write(&ca, certificate.pem()).unwrap();

    let collector = Collector::grpc(Some((certificate.pem(), key.serialize_pem()))).await;
    assert!(collector.endpoint.starts_with("https://"));
    let (node, _) = busy_node(
        &collector,
        &[("OTEL_EXPORTER_OTLP_CERTIFICATE", &ca.display().to_string())],
    )
    .await;
    node.node.flush_otlp().await;
    collector
        .eventually("métricas e traces por gRPC com TLS", |c| {
            !c.metrics().is_empty() && !c.spans().is_empty()
        })
        .await;
    node.shutdown().await;
}

/// UC: UC-OPS-043
#[tokio::test(flavor = "multi_thread")]
async fn messaging_spans_carry_the_semantic_attributes_and_the_topic_tags() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let (node, id) = busy_node(
        &collector,
        &[("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf")],
    )
    .await;
    let got = node
        .client_get("/topics/pedidos/messages?maxMessages=1")
        .await;
    assert_eq!(got.status(), 200);
    let body: Value = got.json().await.unwrap();
    let message = body["messages"][0].clone();
    let ack = node
        .client
        .delete(node.client_url(&format!(
            "/topics/pedidos/messages/{}?receiptToken={}",
            message["id"].as_str().unwrap(),
            message["receiptToken"].as_str().unwrap()
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    node.node.flush_otlp().await;

    let send = collector.wait_span("POST /topics/{topic}/messages").await;
    let attributes = span_attributes(&send);
    assert_eq!(attributes["messaging.system"], "taba");
    assert_eq!(attributes["messaging.destination.name"], "pedidos");
    assert_eq!(attributes["messaging.operation.type"], "send");
    assert_eq!(attributes["messaging.message.id"], id);
    assert_eq!(attributes["taba.topic.tag.team"], "cobranca");
    assert_eq!(attributes["http.route"], "/topics/{topic}/messages");
    assert_eq!(attributes["http.response.status_code"], "201");
    assert!(!attributes.contains_key("taba.durability_event"));

    let receive = collector.wait_span("GET /topics/{topic}/messages").await;
    let attributes = span_attributes(&receive);
    assert_eq!(attributes["messaging.operation.type"], "receive");
    assert_eq!(attributes["messaging.message.id"], id);

    let settle = collector
        .wait_span("DELETE /topics/{topic}/messages/{id}")
        .await;
    let attributes = span_attributes(&settle);
    assert_eq!(attributes["messaging.operation.type"], "settle");
    assert_eq!(attributes["messaging.message.id"], id);
    node.shutdown().await;
}

/// UC: UC-OPS-043
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_operation_span_carries_the_durability_event_mark() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let node = node_with(
        &collector,
        &[("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf")],
    )
    .start()
    .await;
    // Um nó em `failed` responde `503` a uma publicação: erro do servidor, evento de durabilidade.
    create_queue(&node, json!({"name": "falha", "type": "queue"})).await;
    node.node.shared().set_failed();
    let response = node
        .client
        .post(node.client_url("/topics/falha/messages"))
        .header("content-type", "text/plain")
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    node.node.flush_otlp().await;
    collector
        .eventually("span da falha", |c| {
            c.spans().iter().any(|s| {
                span_attributes(s)
                    .get("taba.durability_event")
                    .map(String::as_str)
                    == Some("true")
            })
        })
        .await;
    node.shutdown().await;
}

/// UC: UC-OPS-032
#[tokio::test(flavor = "multi_thread")]
async fn logs_carry_trace_id_span_id_severity_and_the_event_name() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let node = node_with(
        &collector,
        &[("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf")],
    )
    .start()
    .await;
    create_queue(
        &node,
        json!({"name": "dedup", "type": "queue", "fifo": true, "tags": {"team": "cobranca"}}),
    )
    .await;
    let publish_fifo = || async {
        node.client
            .post(node.client_url("/topics/dedup/messages?groupId=g&deduplicationId=d1"))
            .header("content-type", "text/plain")
            .body("a")
            .send()
            .await
            .unwrap()
    };
    assert_eq!(publish_fifo().await.status(), 201);
    assert_eq!(publish_fifo().await.status(), 200, "duplicata");
    node.node.flush_otlp().await;

    collector
        .eventually("o log da deduplicação", |c| {
            c.log_records().iter().any(|r| {
                log_attributes(r).get("event.name").map(String::as_str)
                    == Some("taba.message.deduplicated")
            })
        })
        .await;
    let record = collector
        .log_records()
        .into_iter()
        .find(|r| {
            log_attributes(r).get("event.name").map(String::as_str)
                == Some("taba.message.deduplicated")
        })
        .unwrap();
    assert_eq!(record.severity_text, "DEBUG");
    assert_eq!(record.severity_number, 5, "DEBUG no OpenTelemetry");
    assert_eq!(record.trace_id.len(), 16);
    assert_ne!(hex(&record.trace_id), "0".repeat(32));
    assert_ne!(hex(&record.span_id), "0".repeat(16));
    // O log pertence ao trace da publicação duplicada.
    collector
        .eventually("o span da publicação", |c| {
            c.spans().iter().any(|s| s.trace_id == record.trace_id)
        })
        .await;
    node.shutdown().await;
}

/// UC: UC-OPS-032
#[tokio::test(flavor = "multi_thread")]
async fn the_otlp_log_level_follows_telemetry_logs_level() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let node = node_with(
        &collector,
        &[
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
            ("TABA_TELEMETRY_LOGS_LEVEL", "warn"),
        ],
    )
    .start()
    .await;
    tracing::info!(marker = "info-fora", "não deve sair");
    tracing::warn!(marker = "warn-dentro", "deve sair");
    node.node.flush_otlp().await;
    collector
        .eventually("o log warn", |c| {
            c.log_records()
                .iter()
                .any(|r| log_attributes(r).get("marker").map(String::as_str) == Some("warn-dentro"))
        })
        .await;
    assert!(
        !collector
            .log_records()
            .iter()
            .any(|r| log_attributes(r).get("marker").map(String::as_str) == Some("info-fora"))
    );
    node.shutdown().await;
}

fn traceparent(trace: &str, span: &str, sampled: bool) -> String {
    format!("00-{trace}-{span}-{}", if sampled { "01" } else { "00" })
}

/// UC: UC-OPS-044
#[tokio::test(flavor = "multi_thread")]
async fn a_sampled_traceparent_is_respected_and_the_ratio_sampler_drops_the_rest() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    // `parentbased_traceidratio` com fração 0: sem pai, nada é amostrado; com pai amostrado, vale a
    // decisão do pai.
    let node = node_with(
        &collector,
        &[
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
            ("OTEL_TRACES_SAMPLER", "parentbased_traceidratio"),
            ("OTEL_TRACES_SAMPLER_ARG", "0"),
        ],
    )
    .start()
    .await;
    create_queue(&node, json!({"name": "amostra", "type": "queue"})).await;

    let post = |header: Option<String>| {
        let mut request = node
            .client
            .post(node.client_url("/topics/amostra/messages"))
            .header("content-type", "text/plain")
            .body("x");
        if let Some(value) = header {
            request = request.header("traceparent", value);
        }
        request.send()
    };
    let trace = "4bf92f3577b34da6a3ce929d0e0e4736";
    assert_eq!(post(None).await.unwrap().status(), 201);
    assert_eq!(
        post(Some(traceparent(trace, "00f067aa0ba902b7", true)))
            .await
            .unwrap()
            .status(),
        201
    );
    assert_eq!(
        post(Some(traceparent(
            "11111111111111111111111111111111",
            "00f067aa0ba902b7",
            false
        )))
        .await
        .unwrap()
        .status(),
        201
    );
    node.node.flush_otlp().await;
    collector
        .eventually("o span do pai amostrado", |c| {
            c.spans().iter().any(|s| hex(&s.trace_id) == trace)
        })
        .await;
    let spans = collector.spans();
    let sampled = spans
        .iter()
        .find(|s| hex(&s.trace_id) == trace)
        .expect("span filho do traceparent");
    assert_eq!(hex(&sampled.parent_span_id), "00f067aa0ba902b7");
    // O sem pai (fração 0) e o de pai não amostrado não saem.
    assert!(
        spans
            .iter()
            .all(|s| hex(&s.trace_id) != "11111111111111111111111111111111")
    );
    assert_eq!(
        spans
            .iter()
            .filter(|s| s.name == "POST /topics/{topic}/messages")
            .count(),
        1,
        "só o do pai amostrado"
    );
    node.shutdown().await;
}

/// UC: UC-OPS-044
#[tokio::test(flavor = "multi_thread")]
async fn always_off_exports_no_spans() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let node = node_with(
        &collector,
        &[
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
            ("OTEL_TRACES_SAMPLER", "always_off"),
        ],
    )
    .start()
    .await;
    create_queue(&node, json!({"name": "mudo", "type": "queue"})).await;
    publish(&node, "mudo", "").await;
    node.node.flush_otlp().await;
    collector
        .eventually("as métricas, que seguem saindo", |c| {
            !c.metrics().is_empty()
        })
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(collector.spans().is_empty());
    node.shutdown().await;
}

/// UC: UC-OPS-031, UC-OPS-043
#[tokio::test(flavor = "multi_thread")]
async fn a_cluster_write_is_one_trace_with_a_register_and_a_commit_span_per_node() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let cluster = TestCluster::start_with(
        3,
        &[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
            ("OTEL_TRACES_SAMPLER", "always_on"),
            ("OTEL_METRIC_EXPORT_INTERVAL", "60000"),
        ],
    )
    .await;
    // O ingresso dos nós já é um trace por operação, com um span por nó consultado.
    common::cluster::create_queue(cluster.node(1), "pedidos").await;
    let id = publish(cluster.node(1), "pedidos", "").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for node in &cluster.nodes {
        node.node.flush_otlp().await;
    }
    let root = collector.wait_span("POST /topics/{topic}/messages").await;
    assert_eq!(span_attributes(&root)["messaging.message.id"], id);
    collector
        .eventually("os spans de registro e de commit da escrita", |c| {
            let in_trace = |name: &str| {
                c.spans()
                    .iter()
                    .filter(|s| s.name == name && s.trace_id == root.trace_id)
                    .count()
            };
            in_trace("taba.write.register") >= 2 && in_trace("taba.write.commit") >= 2
        })
        .await;
    let spans = collector.spans();
    for span in spans
        .iter()
        .filter(|s| s.trace_id == root.trace_id && s.name.starts_with("taba.write."))
    {
        assert!(span_attributes(span).contains_key("taba.peer.id"));
        if span.name == "taba.write.register" {
            assert!(span_attributes(span).contains_key("taba.attempt"));
        }
    }
    // O ingresso do terceiro nó: um trace por operação, com um span por nó consultado.
    collector
        .eventually(
            "o trace do ingresso com o registro e o commit nos pares",
            |c| {
                let spans = c.spans();
                spans
                    .iter()
                    .filter(|s| s.name == "taba.cluster.join")
                    .any(|join| {
                        let in_trace = |name: &str| {
                            spans
                                .iter()
                                .any(|s| s.name == name && s.trace_id == join.trace_id)
                        };
                        in_trace("taba.cluster.register") && in_trace("taba.cluster.commit")
                    })
            },
        )
        .await;
    cluster.shutdown().await;
}

/// UC: UC-OPS-031
#[tokio::test(flavor = "multi_thread")]
async fn a_synced_block_is_one_trace_with_a_span_per_batch_naming_the_source_node() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let mut cluster = TestCluster::start_with(
        2,
        &[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
            ("OTEL_TRACES_SAMPLER", "always_on"),
            ("OTEL_METRIC_EXPORT_INTERVAL", "60000"),
        ],
    )
    .await;
    common::cluster::create_queue(cluster.node(1), "carga").await;
    for _ in 0..5 {
        publish(cluster.node(1), "carga", "").await;
    }
    // Um terceiro nó entra e sincroniza os blocos de que passa a ser responsável.
    cluster.add_node(false).await;
    for node in &cluster.nodes {
        node.node.flush_otlp().await;
    }
    collector
        .eventually("os spans de bloco e de busca da sincronização", |c| {
            let spans = c.spans();
            spans
                .iter()
                .filter(|s| s.name == "taba.sync.block")
                .any(|block| {
                    spans.iter().any(|s| {
                        s.name == "taba.sync.fetch"
                            && s.trace_id == block.trace_id
                            && span_attributes(s).contains_key("taba.peer.id")
                    })
                })
        })
        .await;
    let block = collector.span("taba.sync.block").unwrap();
    assert!(span_attributes(&block).contains_key("taba.block.id"));
    cluster.shutdown().await;
}

/// UC: UC-OPS-031
#[tokio::test(flavor = "multi_thread")]
async fn an_exclusion_vote_is_one_trace_with_a_span_per_voter() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let cluster = TestCluster::start_with(
        3,
        &[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
            ("OTEL_TRACES_SAMPLER", "always_on"),
            ("OTEL_METRIC_EXPORT_INTERVAL", "60000"),
        ],
    )
    .await;
    common::cluster::settled(&cluster).await;
    cluster.network.isolate(&common::cluster::host_of(3));
    common::cluster::eventually("os dois saudáveis excluírem o nó 3", || {
        cluster.member_ids(1).len() == 2 && cluster.member_ids(2).len() == 2
    })
    .await;
    for node in &cluster.nodes {
        node.node.flush_otlp().await;
    }
    collector
        .eventually("o trace da votação e o da exclusão", |c| {
            let spans = c.spans();
            spans
                .iter()
                .filter(|s| s.name == "taba.vote.open")
                .any(|vote| {
                    spans
                        .iter()
                        .any(|s| s.name == "taba.vote.request" && s.trace_id == vote.trace_id)
                })
                && spans.iter().any(|s| s.name == "taba.cluster.unregister")
        })
        .await;
    cluster.shutdown().await;
}

/// UC: UC-OPS-031
#[tokio::test(flavor = "multi_thread")]
async fn the_trace_context_travels_to_the_other_nodes_of_a_real_cluster() {
    let _serial = serial().await;
    let collector = Collector::http().await;
    let mut cluster = common::real::RealCluster {
        nodes: Vec::new(),
        tls_dir: None,
    };
    let env = [
        ("OTEL_EXPORTER_OTLP_ENDPOINT", collector.endpoint.as_str()),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
        ("OTEL_TRACES_SAMPLER", "always_on"),
        ("OTEL_METRIC_EXPORT_INTERVAL", "60000"),
    ];
    cluster.add_node(&env).await;
    cluster.add_node(&env).await;
    cluster.settled(2).await;
    common::cluster::create_queue(cluster.node(1), "viagem").await;

    // A publicação entra por um nó; o registro e o commit chegam ao outro por HTTP.
    publish(cluster.node(2), "viagem", "").await;
    for node in &cluster.nodes {
        node.node.flush_otlp().await;
    }
    let root = collector.wait_span("POST /topics/{topic}/messages").await;
    collector
        .eventually("a chamada recebida pelo outro nó, no mesmo trace", |c| {
            c.spans().iter().any(|s| {
                s.name.starts_with("POST /cluster/")
                    && s.trace_id == root.trace_id
                    && s.parent_span_id != root.span_id
            })
        })
        .await;
    cluster.shutdown().await;
}
