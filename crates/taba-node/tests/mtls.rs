//! A segurança da porta do cluster (doc 06, seção 8.0.1; doc 20c, UC-SEG): nós de verdade ligados
//! por TCP em `127.0.0.x`, com o `Transport` HTTP e TLS mútuo. Os certificados vêm de pastas
//! temporárias, e o tempo é o real (a validade dos certificados é do relógio do sistema).

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::real::{
    RealCluster, add_named_pair, anonymous_client, cert_dir, client_of, connect, get, ip_of,
    new_pair, trust_certs_of,
};
use common::{TestNode, series};

async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("requisição");
    assert_eq!(response.status(), 201);
}

/// UC: UC-SEG-007, UC-SEG-011
#[tokio::test(flavor = "multi_thread")]
async fn three_real_nodes_form_a_cluster_over_mutual_tls_and_replicate() {
    let certs = cert_dir();
    let cluster = RealCluster::start(3, Some(certs.path())).await;
    cluster.settled(3).await;

    // Uma fila criada num nó chega aos outros, uma mensagem publicada noutro é consumida no terceiro.
    create_queue(cluster.node(1), "pedidos").await;
    let published = cluster
        .node(2)
        .client
        .post(cluster.node(2).client_url("/topics/pedidos/messages"))
        .json(&json!({"n": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), 201);
    let body: Value = published.json().await.unwrap();
    let id = body["id"].as_str().unwrap().to_owned();
    let consumed = cluster
        .node(3)
        .client_get("/topics/pedidos/messages?maxMessages=1")
        .await;
    assert_eq!(consumed.status(), 200);
    let consumed: Value = consumed.json().await.unwrap();
    assert_eq!(consumed["messages"][0]["id"], id);

    // O certificado do cluster aparece nas métricas (UC-SEG-016).
    let text = cluster.node(1).metrics().await;
    let left =
        series(&text, "taba_cluster_tls_expires_in_seconds").expect("métrica do certificado");
    assert!(left > 29.0 * 86_400.0 && left <= 30.0 * 86_400.0, "{left}");
    cluster.shutdown().await;
}

/// UC: UC-SEG-007
#[tokio::test(flavor = "multi_thread")]
async fn without_the_certificates_folder_the_cluster_port_is_plain_http() {
    let cluster = RealCluster::start(2, None).await;
    cluster.settled(2).await;
    let status = cluster
        .node(1)
        .client
        .get(format!("http://{}/cluster/nodes", cluster.cluster_addr(1)))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, 200);
    cluster.shutdown().await;
}

/// UC: UC-SEG-009, UC-SEG-011
#[tokio::test(flavor = "multi_thread")]
async fn the_handshake_refuses_who_does_not_present_a_certificate_the_node_accepts() {
    let certs = cert_dir();
    let cluster = RealCluster::start(1, Some(certs.path())).await;
    let addr = cluster.cluster_addr(1);

    // Quem apresenta o certificado da pasta é atendido; o nome do host não é conferido (conecta por IP).
    let (accepted, _) = client_of(certs.path());
    let reply = get(addr, accepted, "/cluster/nodes").await.unwrap();
    assert_eq!(reply.status, 200);

    // Sem certificado: o handshake falha antes de qualquer rota.
    let refused = get(addr, anonymous_client(), "/cluster/nodes").await;
    assert!(refused.is_err(), "{refused:?}");

    // Com o certificado de outro cluster (o cliente confia no do servidor, mas o servidor não nele).
    let stranger = tempfile::tempdir().unwrap();
    new_pair(stranger.path(), None, 30);
    trust_certs_of(certs.path(), stranger.path());
    let (stranger_config, _) = client_of(stranger.path());
    let refused = get(addr, stranger_config, "/cluster/nodes").await;
    assert!(refused.is_err(), "{refused:?}");

    // HTTP puro numa porta TLS também não chega a uma rota.
    let plain = reqwest::Client::new()
        .get(format!("http://{addr}/cluster/nodes"))
        .send()
        .await;
    assert!(plain.map_or(true, |r| !r.status().is_success()));
    cluster.shutdown().await;
}

/// UC: UC-SEG-009, UC-SEG-016
#[tokio::test(flavor = "multi_thread")]
async fn an_expired_certificate_is_not_accepted_and_is_not_presented() {
    let certs = cert_dir();
    let cluster = RealCluster::start(1, Some(certs.path())).await;
    let addr = cluster.cluster_addr(1);

    // O cliente só tem um certificado já expirado (gerado há 10 dias, válido por 1): não o
    // apresenta, e o servidor não o aceitaria.
    let old = tempfile::tempdir().unwrap();
    taba_node::secrets::generate_cert(
        old.path(),
        time::OffsetDateTime::now_utc() - time::Duration::days(10),
        1,
        None,
    )
    .unwrap();
    new_pair(old.path(), Some(Duration::from_secs(7 * 86_400)), 30);
    trust_certs_of(certs.path(), old.path());
    let (config, _) = client_of(old.path());
    let refused = get(addr, config, "/cluster/nodes").await;
    assert!(refused.is_err(), "{refused:?}");
    cluster.shutdown().await;
}

/// UC: UC-SEG-017
#[tokio::test(flavor = "multi_thread")]
async fn a_ca_in_the_folder_makes_the_certificates_it_issued_accepted() {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};

    // A CA e um par emitido por ela.
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.distinguished_name.push(DnType::CommonName, "ca");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca, ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(Vec::<String>::new())
        .unwrap()
        .signed_by(&leaf_key, &issuer)
        .unwrap();

    // O nó confia na CA e apresenta o par próprio (autoassinado); o cliente tem o par emitido pela CA.
    let node_certs = cert_dir();
    std::fs::write(node_certs.path().join("ca.crt"), ca_cert.pem()).unwrap();
    let client_certs = tempfile::tempdir().unwrap();
    std::fs::write(client_certs.path().join("eu.crt"), leaf.pem()).unwrap();
    std::fs::write(client_certs.path().join("eu.key"), leaf_key.serialize_pem()).unwrap();
    trust_certs_of(node_certs.path(), client_certs.path());

    let cluster = RealCluster::start(1, Some(node_certs.path())).await;
    let (config, _) = client_of(client_certs.path());
    let reply = get(cluster.cluster_addr(1), config, "/cluster/nodes")
        .await
        .unwrap();
    assert_eq!(reply.status, 200);
    cluster.shutdown().await;
}

/// UC: UC-SEG-010, UC-SEG-013, UC-SEG-014, UC-SEG-015
#[tokio::test(flavor = "multi_thread")]
async fn rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts() {
    let certs = cert_dir();
    // Recarga rápida, para o teste não esperar o intervalo padrão.
    let mut cluster = RealCluster {
        nodes: Vec::new(),
        tls_dir: Some(certs.path().to_owned()),
    };
    cluster
        .add_node(&[("TABA_CLUSTER_TLS_RELOAD_INTERVAL", "200ms")])
        .await;
    let addr = cluster.cluster_addr(1);

    // O cliente conhece o certificado do servidor e já tem o par B, que só começa daqui a 3 s.
    let client_certs = tempfile::tempdir().unwrap();
    trust_certs_of(certs.path(), client_certs.path());
    let b_pem = add_named_pair(client_certs.path(), "b", Some(Duration::from_secs(3)), 30);
    let (client_config, _) = client_of_with_both(&client_certs);

    // Antes do início de B o cliente não tem o que apresentar: recusado. O servidor, por
    // enquanto, apresenta A e mantém uma conexão aberta com ele.
    let (accepted_now, _) = client_of(certs.path());
    let mut open = connect(addr, accepted_now).await.unwrap();
    let before = open.get("/cluster/nodes").await.unwrap();
    assert_eq!(before.status, 200);
    let a_der = before.server_cert.clone();
    assert!(
        get(addr, client_config.clone(), "/cluster/nodes")
            .await
            .is_err()
    );

    // O operador acrescenta B à pasta do servidor: ele passa a confiar em B (sem reiniciar), mas
    // segue apresentando A.
    std::fs::write(certs.path().join("b.crt"), &b_pem).unwrap();
    std::fs::copy(
        client_certs.path().join("b.key"),
        certs.path().join("b.key"),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let (probe, _) = client_of(certs.path());
    let still_a = get(addr, probe.clone(), "/cluster/nodes").await.unwrap();
    assert_eq!(still_a.server_cert, a_der, "ainda apresenta A");

    // Quando a data de B chega, todos passam a apresentar B sozinhos: o cliente é aceito e o
    // servidor apresenta B.
    let mut switched = None;
    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(reply) = get(addr, client_config.clone(), "/cluster/nodes").await
            && reply.status == 200
        {
            switched = Some(reply);
            break;
        }
    }
    let switched = switched.expect("depois do início de B o cliente passa a ser aceito");
    assert_ne!(
        switched.server_cert, a_der,
        "o servidor passou a apresentar B"
    );

    // A conexão que já estava aberta segue com o certificado antigo, e funciona.
    let after = open.get("/cluster/nodes").await.unwrap();
    assert_eq!(after.status, 200);
    assert_eq!(after.server_cert, a_der);

    // Uma recarga com a pasta quebrada é recusada: vale o conjunto anterior.
    let (still, _) = client_of(certs.path());
    std::fs::write(certs.path().join("lixo.crt"), "isto não é PEM").unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        get(addr, still, "/cluster/nodes").await.unwrap().status,
        200,
        "o conjunto anterior seguiu valendo"
    );
    cluster.shutdown().await;
}

fn client_of_with_both(
    dir: &tempfile::TempDir,
) -> (
    rustls::ClientConfig,
    std::sync::Arc<taba_node::cluster::tls::TlsStore>,
) {
    client_of(dir.path())
}

/// UC: UC-SEG-018, UC-CLR-001, UC-SEG-004
#[tokio::test(flavor = "multi_thread")]
async fn only_members_use_the_cluster_routes_and_the_caller_is_the_connection_ip_not_a_header() {
    let certs = cert_dir();
    let cluster = RealCluster::start(2, Some(certs.path())).await;
    cluster.settled(2).await;
    let addr = cluster.cluster_addr(1);

    // Um cliente com o certificado certo, mas de um IP que não está na tabela de membros.
    let (config, _) = client_of(certs.path());
    let outsider = reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .local_address("127.0.0.99".parse::<std::net::IpAddr>().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    for path in [
        "/cluster/status",
        "/cluster/topics",
        "/cluster/tokens",
        "/cluster/blocks/0000000000000000/ids",
        "/cluster/topics/qualquer/idle",
    ] {
        // O cabeçalho do cliente não vale na porta do cluster: o IP é o da conexão.
        let response = outsider
            .get(format!("https://{addr}{path}"))
            .header("x-forwarded-for", ip_of(2))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "{path}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["code"], "cluster.not-member", "{path}");
    }
    // As exceções aceitam quem ainda não é membro.
    let listing = outsider
        .get(format!("https://{addr}/cluster/nodes"))
        .send()
        .await
        .unwrap();
    assert_eq!(listing.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-SEG-006
#[tokio::test(flavor = "multi_thread")]
async fn the_operations_routes_have_no_authentication() {
    // A autenticação só liga em produção; sem `Transport` o nó fica em `bootstrapping` (o
    // `/health/ready` responde `503`), o que não importa aqui.
    let node = TestNode::builder()
        .production()
        .env(
            "TABA_AUTH_ROOT_TOKEN",
            "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001",
        )
        .start()
        .await;
    // A API de cliente exige o token...
    let response = node.client_get("/topics").await;
    assert_eq!(response.status(), 401);
    // ...e as rotas de operação, não.
    for path in ["/health/live", "/health/ready", "/metrics"] {
        let response = node.client_get_ops(path).await;
        assert!(
            ![401, 403].contains(&response.status().as_u16()),
            "{path}: {}",
            response.status()
        );
    }
    assert_eq!(node.client_get_ops("/metrics").await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-SEG-008
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_whose_pair_is_not_valid_stops_the_node_from_starting() {
    // O arquivo existe (a configuração aceita), mas não é um certificado: o nó não sobe.
    let certs = tempfile::tempdir().unwrap();
    std::fs::write(certs.path().join("a.crt"), "não é um certificado").unwrap();
    std::fs::write(certs.path().join("a.key"), "não é uma chave").unwrap();
    let result = TestNode::builder()
        .real(&ip_of(1), "127.0.0.1:1")
        .env("TABA_CLUSTER_TLS_DIR", &certs.path().display().to_string())
        .env("TABA_CLUSTER_FIRST_NODE", "true")
        .try_start()
        .await;
    let error = result.err().expect("o nó não deveria subir").to_string();
    assert!(error.contains("certificados do cluster"), "{error}");
}

/// UC: UC-SEG-008
#[tokio::test(flavor = "multi_thread")]
async fn a_certificates_folder_that_does_not_exist_is_a_start_error() {
    let missing = std::env::temp_dir().join("taba-pasta-que-nao-existe");
    let env = taba_node::config::MapEnv::new([
        ("TABA_MODE", "production"),
        ("TABA_CLUSTER_ENTRY_ADDRESS", "127.0.0.1:8081"),
        ("TABA_CLUSTER_TLS_DIR", missing.to_str().unwrap()),
    ]);
    let error = taba_node::config::Config::parse(None, &env)
        .expect_err("a configuração é recusada")
        .to_string();
    assert!(error.contains("cluster.tls.dir"), "{error}");
}
