use std::time::Duration;

use super::*;
use crate::core::clock::ManualClock;

fn limiter(publish: Option<f64>, burst: Option<u32>) -> (RateLimiter, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::at_doc_epoch());
    let config = RateLimit {
        publish_rps: publish,
        consume_rps: None,
        admin_rps: Some(1.0),
        ops_rps: Some(2.0),
        burst,
    };
    (RateLimiter::from_config(&config, clock.clone()), clock)
}

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

/// UC: UC-LIM-003
#[test]
fn a_burst_is_absorbed_and_then_the_rate_applies() {
    let (limiter, clock) = limiter(Some(2.0), Some(5));
    for _ in 0..5 {
        assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_ok());
    }
    let wait = limiter.check(Category::Publish, ip("1.1.1.1")).unwrap_err();
    assert_eq!(
        wait, 1,
        "meio segundo até o próximo token, arredondado para cima"
    );

    clock.advance(Duration::from_millis(500));
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_ok());
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_err());
}

/// UC: UC-LIM-003
#[test]
fn without_a_burst_the_capacity_is_one_second_of_the_rate() {
    let (limiter, _) = limiter(Some(3.0), None);
    for _ in 0..3 {
        assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_ok());
    }
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_err());
    // Taxa menor que 1 ainda deixa passar uma chamada de cada vez.
    let (slow, _) = limiter_with_admin(0.1);
    assert!(slow.check(Category::Admin, ip("1.1.1.1")).is_ok());
    assert_eq!(slow.check(Category::Admin, ip("1.1.1.1")), Err(10));
}

fn limiter_with_admin(rps: f64) -> (RateLimiter, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::at_doc_epoch());
    let config = RateLimit {
        publish_rps: None,
        consume_rps: None,
        admin_rps: Some(rps),
        ops_rps: None,
        burst: None,
    };
    (RateLimiter::from_config(&config, clock.clone()), clock)
}

/// UC: UC-LIM-001
#[test]
fn the_retry_after_is_the_wait_for_a_whole_token() {
    let (limiter, clock) = limiter_with_admin(0.25);
    assert!(limiter.check(Category::Admin, ip("1.1.1.1")).is_ok());
    assert_eq!(limiter.check(Category::Admin, ip("1.1.1.1")), Err(4));
    clock.advance(Duration::from_secs(3));
    assert_eq!(limiter.check(Category::Admin, ip("1.1.1.1")), Err(1));
    clock.advance(Duration::from_secs(1));
    assert!(limiter.check(Category::Admin, ip("1.1.1.1")).is_ok());
}

/// UC: UC-LIM-002
#[test]
fn categories_and_clients_count_separately() {
    let (limiter, _) = limiter(Some(1.0), None);
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_ok());
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_err());
    assert!(
        limiter.check(Category::Publish, ip("2.2.2.2")).is_ok(),
        "outro IP"
    );
    assert!(
        limiter.check(Category::Admin, ip("1.1.1.1")).is_ok(),
        "outra categoria"
    );
}

/// UC: UC-LIM-001
#[test]
fn a_category_without_a_rate_is_unlimited() {
    let (limiter, _) = limiter(Some(1.0), None);
    for _ in 0..1000 {
        assert!(limiter.check(Category::Consume, ip("1.1.1.1")).is_ok());
    }
    assert_eq!(limiter.tracked(), 0, "nem guarda contador");
}

/// UC: UC-LIM-003
#[test]
fn tokens_never_pile_up_past_the_burst() {
    let (limiter, clock) = limiter(Some(10.0), Some(2));
    clock.advance(Duration::from_secs(3600));
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_ok());
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_ok());
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_err());
    clock.advance(Duration::from_secs(3600));
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_ok());
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_ok());
    assert!(limiter.check(Category::Publish, ip("1.1.1.1")).is_err());
}

/// UC: UC-LIM-005
#[test]
fn idle_counters_are_dropped_when_the_map_grows() {
    let (limiter, clock) = limiter(Some(1.0), None);
    for n in 0..(PRUNE_ABOVE + 10) {
        let a = (n / 250) as u8;
        let b = (n % 250) as u8;
        limiter
            .check(Category::Publish, ip(&format!("10.{a}.{b}.1")))
            .unwrap();
    }
    assert!(limiter.tracked() > PRUNE_ABOVE);
    clock.advance(Duration::from_secs(5)); // todos voltam ao estouro cheio
    limiter.check(Category::Publish, ip("9.9.9.9")).unwrap();
    assert_eq!(limiter.tracked(), 1, "só o contador novo sobrou");
}

/// UC: UC-LIM-002, UC-LIM-009, UC-LIM-010
#[test]
fn routes_are_classified_by_category_and_the_probes_are_exempt() {
    let get = Method::GET;
    let post = Method::POST;
    assert_eq!(
        classify(&post, "/topics/{topic}/messages", None),
        Some(Category::Publish)
    );
    assert_eq!(
        classify(&get, "/topics/{topic}/messages", None),
        Some(Category::Consume)
    );
    assert_eq!(
        classify(&Method::DELETE, "/topics/{topic}/messages/{id}", None),
        Some(Category::Consume)
    );
    assert_eq!(
        classify(
            &Method::PUT,
            "/topics/{topic}/messages/{id}/visibility",
            None
        ),
        Some(Category::Consume)
    );
    assert_eq!(classify(&post, "/topics", None), Some(Category::Admin));
    assert_eq!(
        classify(&Method::PATCH, "/topics/{topic}", None),
        Some(Category::Admin)
    );
    assert_eq!(classify(&post, "/node/drain", None), Some(Category::Admin));

    assert_eq!(classify(&get, "/health/live", None), None);
    assert_eq!(classify(&get, "/health/ready", None), None);
    assert_eq!(classify(&get, "/health/cluster", None), Some(Category::Ops));
    assert_eq!(
        classify(&get, "/health/cluster", Some("scope=cluster")),
        Some(Category::Ops)
    );
    assert_eq!(classify(&get, "/health/cluster", Some("scope=node")), None);
}
