use super::*;

/// UC: UC-CLI-032
#[test]
fn the_connection_config_uses_builtin_values_where_the_profile_is_silent() {
    let c = connection_config(&Profile::default()).unwrap();
    assert_eq!(c.base_url, "http://127.0.0.1:8080");
    assert_eq!(
        (
            c.connect_timeout.as_secs(),
            c.request_timeout.as_secs(),
            c.max_attempts
        ),
        (5, 30, 1)
    );
    assert!(c.token.is_none());
    let mut p = Profile::default();
    p.set("url", "https://t:1").unwrap();
    p.set("token", "tb_x").unwrap();
    p.set("timeout", "7").unwrap();
    let c = connection_config(&p).unwrap();
    assert_eq!(
        (
            c.base_url.as_str(),
            c.token.as_deref(),
            c.request_timeout.as_secs()
        ),
        ("https://t:1", Some("tb_x"), 7)
    );
}

/// UC: UC-CLI-032
#[test]
fn an_unreadable_token_file_or_ca_file_is_reported() {
    let mut p = Profile::default();
    p.set("token-file", "/nao/existe/tok").unwrap();
    assert!(connection_config(&p).unwrap_err().contains("token-file"));
    let mut p = Profile::default();
    p.set("ca-file", "/nao/existe/ca.pem").unwrap();
    assert!(connection_config(&p).unwrap_err().contains("ca-file"));
}
