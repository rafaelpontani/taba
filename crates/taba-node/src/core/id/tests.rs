use super::*;

/// UC: UC-CNV-001
#[test]
fn generated_ids_are_lowercase_canonical_v7() {
    let id = new_at(1_791_288_000_000);
    assert!(is_guid_v7(&id), "{id}");
    assert_eq!(id.len(), 36);
}

#[test]
fn id_carries_its_timestamp() {
    let id = new_at(1_791_288_000_123);
    assert_eq!(unix_millis_of(&id), Some(1_791_288_000_123));
}

#[test]
fn rejects_other_versions_and_uppercase() {
    assert!(!is_guid_v7("550e8400-e29b-41d4-a716-446655440000"));
    assert!(!is_guid_v7(&new_at(1).to_uppercase()));
    assert!(!is_guid_v7("not-an-id"));
    assert_eq!(unix_millis_of("550e8400-e29b-41d4-a716-446655440000"), None);
}

#[test]
fn ids_sort_by_creation_time() {
    let a = new_at(1_000_000);
    let b = new_at(1_000_001);
    assert!(a < b);
}

/// UC: UC-MSG-013
#[test]
fn generator_is_strictly_increasing_even_within_the_same_millisecond() {
    let generator = IdGenerator::new();
    let ids: Vec<String> = (0..1000)
        .map(|_| generator.new_at(1_791_288_000_000))
        .collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "out of order");
    assert!(ids.iter().all(|id| is_guid_v7(id)));
}
