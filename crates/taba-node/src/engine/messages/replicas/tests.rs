use super::*;

#[test]
fn replicas_are_tracked_by_id_and_age() {
    let mut set = ReplicaSet::default();
    assert!(set.is_empty());
    set.insert("b", 20);
    set.insert("a", 10);
    assert_eq!(set.len(), 2);
    assert!(set.contains("a"));
    assert_eq!(set.age_of("b"), Some(20));
    assert_eq!(set.ids(), ["a", "b"], "in age order");

    // Inserting again replaces the age.
    set.insert("a", 30);
    assert_eq!(set.ids(), ["b", "a"]);
    assert_eq!(set.len(), 2);

    assert!(set.remove("a"));
    assert!(!set.remove("a"));
    assert_eq!(set.ids(), ["b"]);
}

#[test]
fn the_ttl_expires_the_replicas_by_age() {
    let mut set = ReplicaSet::default();
    set.insert("old", 100);
    set.insert("new", 900);
    assert_eq!(set.expired(1000, 500), ["old"], "100 + 500 <= 1000");
    assert_eq!(set.expired(1400, 500), ["old", "new"]);
    assert!(set.expired(400, 500).is_empty(), "no past to count");
    assert!(set.expired(599, 500).is_empty());
    assert_eq!(set.expired(600, 500), ["old"]);
}
