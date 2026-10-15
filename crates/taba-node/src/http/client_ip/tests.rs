use axum::http::HeaderValue;

use super::*;

fn rules(enabled: bool, headers: &[&str], proxies: &[&str]) -> ClientIpRules {
    ClientIpRules {
        headers_enabled: enabled,
        headers: headers.iter().map(|h| (*h).to_owned()).collect(),
        trusted_proxies: proxies.iter().map(|p| p.parse().unwrap()).collect(),
    }
}

fn peer(text: &str) -> SocketAddr {
    text.parse().unwrap()
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, HeaderValue::from_str(value).unwrap());
    }
    map
}

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

/// UC: UC-LIM-011
#[test]
fn the_first_address_of_the_first_present_header_wins() {
    let r = rules(true, &["x-real-ip", "x-forwarded-for"], &[]);
    let h = headers(&[
        ("x-forwarded-for", "9.9.9.9, 10.0.0.1"),
        ("x-real-ip", "8.8.8.8"),
    ]);
    assert_eq!(r.resolve(peer("10.0.0.5:1"), &h), ip("8.8.8.8"));

    let r = rules(true, &["x-forwarded-for"], &[]);
    let h = headers(&[("x-forwarded-for", "9.9.9.9, 10.0.0.1")]);
    assert_eq!(r.resolve(peer("10.0.0.5:1"), &h), ip("9.9.9.9"));
}

/// UC: UC-LIM-011
#[test]
fn a_missing_or_invalid_header_falls_back_to_the_connection() {
    let r = rules(true, &["x-forwarded-for"], &[]);
    assert_eq!(
        r.resolve(peer("10.0.0.5:1"), &HeaderMap::new()),
        ip("10.0.0.5")
    );
    let h = headers(&[("x-forwarded-for", "not-an-ip")]);
    assert_eq!(r.resolve(peer("10.0.0.5:1"), &h), ip("10.0.0.5"));
}

/// UC: UC-LIM-011
#[test]
fn addresses_with_a_port_and_ipv6_are_understood() {
    let r = rules(true, &["x-forwarded-for"], &[]);
    let with_port = headers(&[("x-forwarded-for", "1.2.3.4:5678")]);
    assert_eq!(r.resolve(peer("10.0.0.5:1"), &with_port), ip("1.2.3.4"));
    let v6 = headers(&[("x-forwarded-for", "[2001:db8::1]:80")]);
    assert_eq!(r.resolve(peer("10.0.0.5:1"), &v6), ip("2001:db8::1"));
}

/// UC: UC-LIM-011
#[test]
fn headers_are_only_trusted_from_the_trusted_proxies() {
    let r = rules(true, &["x-forwarded-for"], &["10.0.0.0/8"]);
    let h = headers(&[("x-forwarded-for", "9.9.9.9")]);
    assert_eq!(r.resolve(peer("10.1.2.3:1"), &h), ip("9.9.9.9"));
    assert_eq!(
        r.resolve(peer("203.0.113.7:1"), &h),
        ip("203.0.113.7"),
        "outside the list the header is ignored (whoever forges it does not escape the limit)"
    );
}

/// UC: UC-LIM-012
#[test]
fn disabled_headers_always_use_the_connection() {
    let r = rules(false, &["x-forwarded-for"], &[]);
    let h = headers(&[("x-forwarded-for", "9.9.9.9")]);
    assert_eq!(r.resolve(peer("10.0.0.5:1"), &h), ip("10.0.0.5"));
}

#[test]
fn an_ipv4_mapped_connection_is_the_ipv4_address() {
    let r = rules(false, &[], &[]);
    assert_eq!(
        r.resolve(peer("[::ffff:10.0.0.5]:1"), &HeaderMap::new()),
        ip("10.0.0.5")
    );
}
