use std::collections::BTreeSet;
use std::time::Duration;

use super::*;

const PROD: [(&str, &str); 1] = [("TABA_CLUSTER_ENTRY_ADDRESS", "taba-lb.interno:8081")];

fn parse_env(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
    let env = MapEnv(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    );
    Config::parse(None, &env)
}

fn prod_with(extra: &[(&str, &str)]) -> Result<Config, ConfigError> {
    let mut pairs = PROD.to_vec();
    pairs.extend_from_slice(extra);
    parse_env(&pairs)
}

fn problems(result: Result<Config, ConfigError>) -> String {
    match result {
        Err(ConfigError::Validation(list)) => list.join("\n"),
        other => panic!("esperava erro de validação, veio {other:?}"),
    }
}

/// UC: UC-CFG-002
#[test]
fn documented_defaults_apply_when_nothing_is_set() {
    let c = prod_with(&[]).unwrap();
    assert_eq!(c.mode, Mode::Production);
    assert_eq!(
        (c.ports.api, c.ports.cluster, c.ports.operations),
        (8080, 8081, None)
    );
    assert_eq!(c.write.replication_factor, 3);
    assert_eq!(c.write.quorum, 2);
    assert_eq!(
        c.delivery.visibility_timeout_default,
        Duration::from_secs(120)
    );
    assert_eq!(c.delivery.max_messages_default, 10);
    assert_eq!(c.delivery.ready_list_size, 1000);
    assert_eq!(c.delivery.fair_ready_weight, 0.1);
    assert_eq!(c.delivery.fair_normal_every, 10);
    assert_eq!(c.retention.ttl_default, Duration::from_secs(1_209_600));
    assert_eq!(c.retention.max_deliveries_default, 10);
    assert_eq!(c.retention.sweep_interval, Duration::from_secs(60));
    assert_eq!(c.fifo.dedup_window, Duration::from_secs(300));
    assert_eq!(
        (c.cluster.vote_quorum_percent, c.cluster.vote_min_voters),
        (51, 2)
    );
    assert_eq!(c.cluster.notify_window, Duration::from_millis(100));
    assert_eq!(c.cluster.bootstrap_retry_max_attempts, 0);
    assert!(!c.cluster.first_node);
    assert_eq!(c.messages.max_bytes, 5_242_880);
    assert_eq!(c.messages.hard_limit, None);
    assert_eq!(c.topics.dead_letter_suffix, "-dead-letter");
    assert_eq!(
        c.topics.volatile_idle_timeout_default,
        Duration::from_secs(600)
    );
    assert_eq!(c.storage.rate_window, Duration::from_secs(300));
    assert_eq!(c.storage.topic_reserve, 1000);
    assert_eq!(c.storage.db_synchronous, DbSynchronous::Normal);
    assert_eq!(c.tls.reload_interval, Duration::from_secs(60));
    assert_eq!(c.tls.expiry_warning, Duration::from_secs(30 * 86_400));
    assert!(c.drain.on_stop);
    assert!(c.telemetry.prometheus_enabled);
    assert_eq!(c.telemetry.topics_mode, TopicsMetricsMode::Off);
    assert_eq!(c.stats.topics_top_groups, 10);
    assert_eq!(c.stats.topics_noisy_group_share, 50);
    assert!(c.client_ip.headers_enabled);
    assert_eq!(c.client_ip.headers, ["X-Forwarded-For"]);
    assert!(c.client_ip.trusted_proxies.is_empty() && c.operations_allowed_ips.is_empty());
    assert!(
        !c.auth.enabled,
        "sem token raiz a autenticação fica desligada"
    );
}

/// UC: UC-CFG-002
#[test]
fn quorum_defaults_to_the_majority_of_the_replication_factor() {
    for (factor, quorum) in [("1", 1), ("2", 2), ("3", 2), ("4", 3), ("5", 3)] {
        let c = prod_with(&[("TABA_REPLICATION_FACTOR", factor)]).unwrap();
        assert_eq!(c.write.quorum, quorum, "fator {factor}");
    }
}

/// UC: UC-CFG-003
#[test]
fn without_limit_means_the_ceiling_does_not_exist_until_set() {
    let c = prod_with(&[]).unwrap();
    assert_eq!(c.topics.distributor_max_targets, None);
    assert_eq!(c.topics.volatile_max_count, None);

    let c = prod_with(&[
        ("TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS", "50"),
        ("TABA_TOPICS_VOLATILE_MAX_COUNT", "7"),
    ])
    .unwrap();
    assert_eq!(c.topics.distributor_max_targets, Some(50));
    assert_eq!(c.topics.volatile_max_count, Some(7));
}

#[test]
fn quorum_below_the_majority_is_accepted_but_out_of_range_is_not() {
    let c = prod_with(&[("TABA_REPLICATION_FACTOR", "3"), ("TABA_WRITE_QUORUM", "1")]).unwrap();
    assert_eq!(c.write.quorum, 1);
    assert!(problems(prod_with(&[("TABA_WRITE_QUORUM", "0")])).contains("write.quorum"));
    assert!(problems(prod_with(&[("TABA_WRITE_QUORUM", "4")])).contains("write.quorum"));
}

#[test]
fn production_needs_the_entry_address_and_development_does_not() {
    assert!(problems(parse_env(&[])).contains("cluster.entry-address"));
    let dev = parse_env(&[("TABA_MODE", "development")]).unwrap();
    assert!(dev.is_development());
    assert_eq!(dev.cluster.entry_address, None);
}

#[test]
fn development_forces_a_single_node_and_warns_about_ignored_parameters() {
    let c = parse_env(&[
        ("TABA_MODE", "development"),
        ("TABA_REPLICATION_FACTOR", "5"),
        ("TABA_WRITE_QUORUM", "4"),
        ("TABA_CLUSTER_FIRST_NODE", "true"),
        ("TABA_RING_VIRTUAL_NODES", "16"),
        ("TABA_AUTH_ENABLED", "true"),
    ])
    .unwrap();
    assert_eq!((c.write.replication_factor, c.write.quorum), (1, 1));
    assert!(
        !c.auth.enabled,
        "em desenvolvimento a autenticação é sempre desligada"
    );
    let warning = c.warnings.join("\n");
    for key in [
        "replication.factor",
        "write.quorum",
        "cluster.first-node",
        "ring.virtual-nodes",
    ] {
        assert!(
            warning.contains(key),
            "{key} deveria constar do aviso: {warning}"
        );
    }
}

#[test]
fn the_three_ports_must_differ() {
    assert!(
        problems(prod_with(&[
            ("TABA_API_PORT", "9000"),
            ("TABA_CLUSTER_PORT", "9000")
        ]))
        .contains("api.port")
    );
    assert!(
        problems(prod_with(&[
            ("TABA_API_PORT", "9000"),
            ("TABA_OPERATIONS_PORT", "9000")
        ]))
        .contains("operations.port")
    );
    assert!(
        problems(prod_with(&[
            ("TABA_CLUSTER_PORT", "9000"),
            ("TABA_OPERATIONS_PORT", "9000")
        ]))
        .contains("cluster.port")
    );
    // `0` (porta livre qualquer, usada nos testes) nunca é repetida.
    assert!(
        prod_with(&[
            ("TABA_API_PORT", "0"),
            ("TABA_CLUSTER_PORT", "0"),
            ("TABA_OPERATIONS_PORT", "0")
        ])
        .is_ok()
    );
    // A porta do cluster não é aberta em desenvolvimento, então não conflita.
    let dev = parse_env(&[
        ("TABA_MODE", "development"),
        ("TABA_API_PORT", "9000"),
        ("TABA_CLUSTER_PORT", "9000"),
    ]);
    assert!(dev.is_ok());
}

/// UC: UC-CFG-005
#[test]
fn default_message_size_cannot_pass_the_hard_limit() {
    assert!(prod_with(&[("TABA_MESSAGE_HARD_LIMIT", "10MB")]).is_ok());
    assert!(
        prod_with(&[("TABA_MESSAGE_HARD_LIMIT", "5MB")]).is_ok(),
        "igual ao limite vale"
    );
    assert!(
        problems(prod_with(&[("TABA_MESSAGE_HARD_LIMIT", "1MB")])).contains("message.hard-limit")
    );
}

/// UC: UC-CFG-005
#[test]
fn stream_line_limit_must_fit_the_hard_limit_in_base64() {
    // 1 MB em base64 são 1_398_104 bytes, mais o envelope.
    let base = [
        ("TABA_MESSAGE_MAX_BYTES", "1MB"),
        ("TABA_MESSAGE_HARD_LIMIT", "1MB"),
    ];
    let mut ok = base.to_vec();
    ok.push(("TABA_CLUSTER_SYNC_STREAM_MAX_LINE_BYTES", "1500000"));
    assert!(prod_with(&ok).is_ok());

    let mut small = base.to_vec();
    small.push(("TABA_CLUSTER_SYNC_STREAM_MAX_LINE_BYTES", "1MB"));
    assert!(problems(prod_with(&small)).contains("max-line-bytes"));
}

/// UC: UC-CFG-008
#[test]
fn wrong_type_stops_startup_naming_the_parameter() {
    for (var, key) in [
        ("TABA_API_PORT", "api.port"),
        ("TABA_REPLICATION_FACTOR", "replication.factor"),
        (
            "TABA_DELIVERY_VISIBILITY_TIMEOUT_DEFAULT",
            "delivery.visibility-timeout.default",
        ),
        ("TABA_MESSAGE_MAX_BYTES", "message.max-bytes"),
        ("TABA_DRAIN_ON_STOP", "drain.on-stop"),
        ("TABA_MODE", "mode"),
    ] {
        let message = prod_with(&[(var, "abc")]).unwrap_err().to_string();
        assert!(
            message.contains(key) && message.contains(var),
            "{var}: {message}"
        );
    }
}

#[test]
fn allowed_ips_must_be_valid_addresses_or_cidr() {
    let c = prod_with(&[("TABA_OPERATIONS_ALLOWED_IPS", "10.0.0.0/8, 192.168.1.10")]).unwrap();
    assert_eq!(c.operations_allowed_ips.len(), 2);
    let err = prod_with(&[("TABA_OPERATIONS_ALLOWED_IPS", "10.0.0.0/8, banana")])
        .unwrap_err()
        .to_string();
    assert!(err.contains("operations.allowed-ips"), "{err}");
}

#[test]
fn storage_dir_must_end_in_taba() {
    assert!(prod_with(&[("TABA_STORAGE_DIR", "/dados/taba")]).is_ok());
    assert!(problems(prod_with(&[("TABA_STORAGE_DIR", "/dados/outra")])).contains("storage.dir"));
}

#[test]
fn ranges_from_the_fixed_limits_apply_to_the_defaults() {
    assert!(
        problems(prod_with(&[("TABA_RETENTION_TTL_DEFAULT", "30s")]))
            .contains("retention.ttl.default")
    );
    assert!(
        problems(prod_with(&[(
            "TABA_RETENTION_MAX_DELIVERIES_DEFAULT",
            "1001"
        )]))
        .contains("max-deliveries")
    );
    assert!(
        problems(prod_with(&[(
            "TABA_TOPICS_DEAD_LETTER_SUFFIX",
            "com espaco"
        )]))
        .contains("dead-letter.suffix")
    );
    assert!(
        problems(prod_with(&[("TABA_DELIVERY_FAIR_READY_WEIGHT", "1.5")])).contains("ready-weight")
    );
}

mod secrets {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const TOKEN_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TOKEN_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn with_secrets(dir: &TempDir, extra: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let mut pairs = vec![("TABA_SECRETS_DIR", dir.path().to_str().unwrap())];
        pairs.extend_from_slice(extra);
        prod_with(&pairs)
    }

    #[test]
    fn auth_turns_on_by_itself_when_the_secrets_folder_has_a_root_token() {
        let dir = TempDir::new().unwrap();
        assert!(
            !with_secrets(&dir, &[]).unwrap().auth.enabled,
            "sem root-token fica desligada"
        );

        fs::write(
            dir.path().join("root-token"),
            format!("{TOKEN_A}\n{TOKEN_B}\n"),
        )
        .unwrap();
        let c = with_secrets(&dir, &[]).unwrap();
        assert!(c.auth.enabled);
        assert_eq!(
            c.auth.root_token,
            RootToken::File(dir.path().join("root-token"))
        );

        let off = with_secrets(&dir, &[("TABA_AUTH_ENABLED", "false")]).unwrap();
        assert!(
            !off.auth.enabled,
            "auth.enabled=false força desligada, mesmo com a pasta montada"
        );
    }

    #[test]
    fn inline_root_token_enables_auth_and_never_shows_in_debug() {
        let c = prod_with(&[("TABA_AUTH_ROOT_TOKEN", TOKEN_A)]).unwrap();
        assert!(c.auth.enabled);
        assert!(
            !format!("{c:?}").contains(TOKEN_A),
            "o segredo vazou no Debug"
        );
        let RootToken::Inline(secret) = &c.auth.root_token else {
            panic!("esperava token inline")
        };
        assert_eq!(secret.expose(), TOKEN_A);
    }

    #[test]
    fn short_root_token_stops_startup() {
        assert!(
            problems(prod_with(&[("TABA_AUTH_ROOT_TOKEN", "curto")])).contains("auth.root-token")
        );
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("root-token"), "curto\n").unwrap();
        assert!(problems(with_secrets(&dir, &[])).contains("root-token"));
        fs::write(
            dir.path().join("root-token"),
            format!("{TOKEN_A}\n{TOKEN_B}\n{TOKEN_A}\n"),
        )
        .unwrap();
        assert!(
            problems(with_secrets(&dir, &[])).contains("root-token"),
            "no máximo duas linhas"
        );
    }

    #[test]
    fn forcing_auth_without_a_root_token_stops_startup() {
        assert!(
            problems(prod_with(&[("TABA_AUTH_ENABLED", "true")])).contains("auth.enabled=true")
        );
    }

    #[test]
    fn a_named_root_token_file_that_does_not_exist_stops_startup() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nao-existe");
        let err = problems(prod_with(&[(
            "TABA_AUTH_ROOT_TOKEN_FILE",
            missing.to_str().unwrap(),
        )]));
        assert!(err.contains("auth.root-token-file"), "{err}");
    }

    #[test]
    fn tls_folder_is_found_by_default_and_needs_a_cert_pair() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            with_secrets(&dir, &[]).unwrap().tls.dir,
            None,
            "sem pasta: HTTP puro com aviso"
        );

        let tls = dir.path().join("tls");
        fs::create_dir(&tls).unwrap();
        assert!(
            problems(with_secrets(&dir, &[])).contains("cluster.tls.dir"),
            "pasta vazia não tem par válido"
        );

        fs::write(tls.join("cluster-2026.crt"), "cert").unwrap();
        assert!(
            problems(with_secrets(&dir, &[])).contains("cluster.tls.dir"),
            ".crt sem .key só serve para confiar"
        );

        fs::write(tls.join("cluster-2026.key"), "key").unwrap();
        assert_eq!(with_secrets(&dir, &[]).unwrap().tls.dir, Some(tls));
    }

    #[test]
    fn a_named_tls_folder_that_does_not_exist_stops_startup() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nao-existe");
        let err = problems(prod_with(&[(
            "TABA_CLUSTER_TLS_DIR",
            missing.to_str().unwrap(),
        )]));
        assert!(err.contains("cluster.tls.dir"), "{err}");
    }

    #[test]
    fn development_ignores_tls_and_secrets() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("tls")).unwrap();
        let c = parse_env(&[
            ("TABA_MODE", "development"),
            ("TABA_SECRETS_DIR", dir.path().to_str().unwrap()),
        ])
        .unwrap();
        assert_eq!(c.tls.dir, None);
        assert!(!c.auth.enabled);
    }
}

#[test]
fn file_values_load_and_environment_overrides_them() {
    let yaml = "mode: production\napi:\n  port: 9100\ncluster:\n  port: 9101\n  entry-address: lb:9101\nreplication:\n  factor: 5\n";
    let env = MapEnv::new([("TABA_API_PORT", "9200")]);
    let c = Config::parse(Some(("taba.yml", yaml)), &env).unwrap();
    assert_eq!((c.ports.api, c.ports.cluster), (9200, 9101));
    assert_eq!((c.write.replication_factor, c.write.quorum), (5, 3));
    assert_eq!(c.cluster.entry_address.as_deref(), Some("lb:9101"));
}

/// UC: UC-CFG-010
#[test]
fn an_explicit_config_file_that_does_not_exist_stops_startup() {
    let err = Config::load(Some(std::path::Path::new("/nao/existe/taba.yml"))).unwrap_err();
    assert!(matches!(err, ConfigError::Read { .. }), "{err}");
    assert!(err.to_string().contains("taba.yml"));
}

#[test]
fn every_registered_parameter_is_read_by_the_typed_config() {
    // Mantém a tabela de `params` e o `Config` em sincronia: parâmetro na tabela que
    // ninguém lê é configuração que não faz nada.
    let env = MapEnv(
        PROD.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    );
    let (source, mut warnings) = Source::build(None, &env).unwrap();
    Config::build(&source, &mut warnings).unwrap();
    let registered: BTreeSet<String> = PARAMS.iter().map(|p| p.key.to_owned()).collect();
    let read = source.keys_read();
    let missing: Vec<_> = registered.difference(&read).collect();
    assert!(
        missing.is_empty(),
        "parâmetros que o Config não lê: {missing:?}"
    );
}
