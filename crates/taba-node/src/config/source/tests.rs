use super::*;

fn d(s: &str) -> Option<Duration> {
    Duration::parse(s)
}

/// UC: UC-CFG-012
#[test]
fn durations_need_a_unit() {
    assert_eq!(d("100ms"), Some(Duration::from_millis(100)));
    assert_eq!(d("60s"), Some(Duration::from_secs(60)));
    assert_eq!(d("5m"), Some(Duration::from_secs(300)));
    assert_eq!(d("2h"), Some(Duration::from_secs(7200)));
    assert_eq!(d("14d"), Some(Duration::from_secs(14 * 86_400)));
    for bad in ["60", "s", "", "1.5s", "10x", "-5s", "5 s"] {
        assert_eq!(d(bad), None, "{bad:?}");
    }
}

/// UC: UC-CFG-012
#[test]
fn sizes_are_binary_and_accept_plain_bytes() {
    assert_eq!(Bytes::parse("5MB"), Some(Bytes(5_242_880)));
    assert_eq!(Bytes::parse("5mb"), Some(Bytes(5_242_880)));
    assert_eq!(Bytes::parse("512KB"), Some(Bytes(524_288)));
    assert_eq!(Bytes::parse("1GB"), Some(Bytes(1_073_741_824)));
    assert_eq!(Bytes::parse("5242880"), Some(Bytes(5_242_880)));
    assert_eq!(Bytes::parse("100B"), Some(Bytes(100)));
    for bad in ["MB", "5XB", "", "-1", "5 MB"] {
        assert_eq!(Bytes::parse(bad), None, "{bad:?}");
    }
    assert_eq!(Bytes::parse("99999999999999TB"), None, "u64 overflow");
}

#[test]
fn lists_trim_and_skip_empty_items() {
    assert_eq!(
        List::parse(" a , b,,c "),
        Some(List(vec!["a".into(), "b".into(), "c".into()]))
    );
    assert_eq!(List::parse(""), Some(List(vec![])));
}

#[test]
fn net_lists_accept_hosts_and_cidr() {
    let nets = NetList::parse("10.0.0.0/8, 192.168.1.10, ::1").unwrap().0;
    assert_eq!(nets.len(), 3);
    assert_eq!(nets[1].to_string(), "192.168.1.10/32");
    assert!(NetList::parse("10.0.0.0/33").is_none());
    assert!(NetList::parse("nao-e-ip").is_none());
}

#[test]
fn booleans_are_strict() {
    assert_eq!(bool::parse("TRUE"), Some(true));
    assert_eq!(bool::parse("false"), Some(false));
    assert_eq!(bool::parse("1"), None);
    assert_eq!(bool::parse("sim"), None);
}

/// UC: UC-CFG-009
#[test]
fn environment_wins_over_file_and_file_over_default() {
    let yaml = "api:\n  port: 9000\ncluster:\n  port: 9001\n";
    let env = MapEnv::new([("TABA_API_PORT", "7000")]);
    let (source, _) = Source::build(Some(("taba.yml", yaml)), &env).unwrap();
    assert_eq!(
        source.get::<u16>("api.port").unwrap(),
        7000,
        "environment beats the file"
    );
    assert_eq!(
        source.get::<u16>("cluster.port").unwrap(),
        9001,
        "file beats the default"
    );
    assert_eq!(
        source.get::<u32>("replication.factor").unwrap(),
        3,
        "table default"
    );
}

/// UC: UC-CFG-009
#[test]
fn empty_environment_value_counts_as_unset() {
    let env = MapEnv::new([("TABA_API_PORT", "  ")]);
    let (source, _) = Source::build(None, &env).unwrap();
    assert_eq!(source.get::<u16>("api.port").unwrap(), 8080);
    assert!(!source.is_set("api.port"));
}

#[test]
fn yaml_lists_and_dotted_keys_flatten_like_nested_ones() {
    let yaml = "operations.allowed-ips: [10.0.0.0/8, 192.168.1.10]\nclient-ip:\n  headers: [X-Real-IP, X-Forwarded-For]\n  headers.enabled: false\n";
    let (source, _) = Source::build(Some(("taba.yml", yaml)), &MapEnv::default()).unwrap();
    assert_eq!(
        source
            .get::<NetList>("operations.allowed-ips")
            .unwrap()
            .0
            .len(),
        2
    );
    assert_eq!(
        source.get::<List>("client-ip.headers").unwrap().0,
        ["X-Real-IP", "X-Forwarded-For"]
    );
    assert!(!source.get::<bool>("client-ip.headers.enabled").unwrap());
}

/// UC: UC-CFG-011
#[test]
fn unknown_yaml_key_is_rejected_by_name() {
    let err = Source::build(Some(("taba.yml", "api:\n  prot: 80\n")), &MapEnv::default())
        .err()
        .unwrap();
    assert!(
        matches!(&err, ConfigError::UnknownKey(k) if k == "api.prot"),
        "{err}"
    );
}

#[test]
fn broken_yaml_names_the_file() {
    let err = Source::build(Some(("taba.yml", "api: [")), &MapEnv::default())
        .err()
        .unwrap();
    assert!(err.to_string().contains("taba.yml"), "{err}");
}

/// UC: UC-CFG-011
#[test]
fn unknown_taba_variable_becomes_a_warning() {
    let env = MapEnv::new([
        ("TABA_API_PROT", "80"),
        ("TABA_CONFIG_FILE", "x.yml"),
        ("OTHER", "1"),
    ]);
    let (_, warnings) = Source::build(None, &env).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("TABA_API_PROT"));
}

/// UC: UC-CFG-008
#[test]
fn invalid_value_names_the_parameter_and_its_variable() {
    let env = MapEnv::new([("TABA_API_PORT", "oito mil")]);
    let (source, _) = Source::build(None, &env).unwrap();
    let err = source.get::<u16>("api.port").unwrap_err().to_string();
    assert!(err.contains("api.port"), "{err}");
    assert!(err.contains("TABA_API_PORT"), "{err}");
    assert!(err.contains("oito mil"), "{err}");
}
