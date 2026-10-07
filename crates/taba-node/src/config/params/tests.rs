use super::*;
use std::collections::HashSet;

/// UC: UC-CFG-001
#[test]
fn env_name_follows_the_documented_rule() {
    assert_eq!(
        env_name("topics.distributor.max-targets"),
        "TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS"
    );
    assert_eq!(env_name("mode"), "TABA_MODE");
    assert_eq!(
        env_name("cluster.entry-address"),
        "TABA_CLUSTER_ENTRY_ADDRESS"
    );
    assert_eq!(
        env_name("client-ip.trusted-proxies"),
        "TABA_CLIENT_IP_TRUSTED_PROXIES"
    );
}

#[test]
fn keys_and_env_names_are_unique() {
    let keys: HashSet<_> = PARAMS.iter().map(|p| p.key).collect();
    assert_eq!(keys.len(), PARAMS.len(), "chave repetida");
    // `-` e `.` viram `_`: duas chaves podem colidir na variável de ambiente.
    let envs: HashSet<_> = PARAMS.iter().map(|p| env_name(p.key)).collect();
    assert_eq!(
        envs.len(),
        PARAMS.len(),
        "duas chaves com a mesma variável de ambiente"
    );
    assert!(
        !envs.contains("TABA_CONFIG_FILE"),
        "TABA_CONFIG_FILE é reservada ao caminho do arquivo"
    );
}
