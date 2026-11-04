use std::io::Write as _;

use crate::config::{Config, MapEnv};

use super::*;

fn config_with(env: &[(&str, &str)]) -> Config {
    let mut pairs = vec![
        ("TABA_REPLICATION_FACTOR", "1"),
        ("TABA_CLUSTER_TLS_ENABLED", "false"),
    ];
    pairs.extend_from_slice(env);
    Config::parse(None, &MapEnv::from_pairs(&pairs)).expect("valid configuration")
}

/// UC: UC-GTW-012
#[test]
fn the_gateway_is_off_by_default_and_has_the_documented_ports() {
    let config = config_with(&[]);
    assert!(!config.gateway.sqs.enabled);
    assert_eq!(config.gateway.sqs.port, 9324);
    assert!(config.gateway.sqs.public_url.is_none());
    assert!(config.gateway.credentials_file.is_none());
}

/// UC: UC-GTW-012
#[test]
fn the_keys_of_the_gateway_are_read_from_the_environment() {
    let config = config_with(&[
        ("TABA_GATEWAY_SQS_ENABLED", "true"),
        ("TABA_GATEWAY_SQS_PORT", "19324"),
        ("TABA_GATEWAY_SQS_PUBLIC_URL", "https://queues.example.com/"),
        ("TABA_GATEWAY_AWS_CREDENTIALS_FILE", "/run/secrets/aws.json"),
    ]);
    assert!(config.gateway.sqs.enabled);
    assert_eq!(config.gateway.sqs.port, 19324);
    assert_eq!(
        config.gateway.sqs.public_url.as_deref(),
        Some("https://queues.example.com")
    );
    assert_eq!(
        config.gateway.credentials_file.as_deref(),
        Some(std::path::Path::new("/run/secrets/aws.json"))
    );
}

/// UC: UC-GTW-012
#[test]
fn the_port_of_the_gateway_cannot_repeat_another_port_of_the_node() {
    let result = Config::parse(
        None,
        &MapEnv::from_pairs(&[
            ("TABA_REPLICATION_FACTOR", "1"),
            ("TABA_CLUSTER_TLS_ENABLED", "false"),
            ("TABA_GATEWAY_SQS_ENABLED", "true"),
            ("TABA_GATEWAY_SQS_PORT", "8080"),
        ]),
    );
    assert!(result.is_err(), "9324 -> 8080 repeats api.port");
    let off = Config::parse(
        None,
        &MapEnv::from_pairs(&[
            ("TABA_REPLICATION_FACTOR", "1"),
            ("TABA_CLUSTER_TLS_ENABLED", "false"),
            ("TABA_GATEWAY_SQS_PORT", "8080"),
        ]),
    );
    assert!(off.is_ok(), "a port that is not used does not conflict");
}

/// UC: UC-GTW-029
#[test]
fn the_sns_gateway_is_off_by_default_and_its_keys_are_read_from_the_environment() {
    let off = config_with(&[]);
    assert!(!off.gateway.sns.enabled);
    assert_eq!(off.gateway.sns.port, 9325);
    let on = config_with(&[
        ("TABA_GATEWAY_SNS_ENABLED", "true"),
        ("TABA_GATEWAY_SNS_PORT", "19325"),
    ]);
    assert!(on.gateway.sns.enabled);
    assert_eq!(on.gateway.sns.port, 19325);
    assert!(!on.gateway.sqs.enabled, "each protocol turns on by itself");
}

/// UC: UC-GTW-029
#[test]
fn the_port_of_sns_cannot_repeat_another_port_of_the_node_or_the_sqs_port() {
    let parse = |pairs: &[(&str, &str)]| {
        let mut all = vec![
            ("TABA_REPLICATION_FACTOR", "1"),
            ("TABA_CLUSTER_TLS_ENABLED", "false"),
        ];
        all.extend_from_slice(pairs);
        Config::parse(None, &MapEnv::from_pairs(&all))
    };
    assert!(
        parse(&[
            ("TABA_GATEWAY_SNS_ENABLED", "true"),
            ("TABA_GATEWAY_SNS_PORT", "8080")
        ])
        .is_err(),
        "9325 -> 8080 repeats api.port"
    );
    assert!(
        parse(&[
            ("TABA_GATEWAY_SQS_ENABLED", "true"),
            ("TABA_GATEWAY_SNS_ENABLED", "true"),
            ("TABA_GATEWAY_SNS_PORT", "9324"),
        ])
        .is_err(),
        "the two protocols cannot share a port"
    );
    assert!(
        parse(&[("TABA_GATEWAY_SNS_PORT", "9324")]).is_ok(),
        "a port that is not used does not conflict"
    );
    assert!(
        parse(&[
            ("TABA_GATEWAY_SQS_ENABLED", "true"),
            ("TABA_GATEWAY_SQS_PORT", "0"),
            ("TABA_GATEWAY_SNS_ENABLED", "true"),
            ("TABA_GATEWAY_SNS_PORT", "0"),
        ])
        .is_ok(),
        "port 0 (any free port) never conflicts"
    );
}

/// UC: UC-GTW-012
#[test]
fn a_public_url_without_a_scheme_is_refused() {
    let result = Config::parse(
        None,
        &MapEnv::from_pairs(&[
            ("TABA_REPLICATION_FACTOR", "1"),
            ("TABA_CLUSTER_TLS_ENABLED", "false"),
            ("TABA_GATEWAY_SQS_ENABLED", "true"),
            ("TABA_GATEWAY_SQS_PUBLIC_URL", "queues.example.com"),
        ]),
    );
    assert!(result.is_err());
}

/// UC: UC-GTW-001
#[test]
fn the_credentials_file_is_optional_but_an_invalid_one_stops_the_start() {
    let none = open_credentials(&config_with(&[])).expect("no file");
    assert!(none.is_empty());

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("aws.json");
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(b"{ not json"))
        .expect("write");
    let config = config_with(&[(
        "TABA_GATEWAY_AWS_CREDENTIALS_FILE",
        path.to_str().expect("utf8"),
    )]);
    assert!(open_credentials(&config).is_err());

    let missing = config_with(&[("TABA_GATEWAY_AWS_CREDENTIALS_FILE", "/nonexistent/aws.json")]);
    assert!(open_credentials(&missing).is_err());
}
