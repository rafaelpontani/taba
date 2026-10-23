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

/// UC: UC-FIF-049
#[test]
fn a_generator_with_a_floor_never_goes_below_it_and_reports_its_watermark() {
    let generator = IdGenerator::new();
    assert_eq!(generator.watermark(), None);
    generator.raise_floor(2_000_000);
    // An instant before the floor is lifted to it.
    let id = generator.new_at(1_000_000);
    assert_eq!(unix_millis_of(&id), Some(2_000_000));
    let later = generator.new_at(3_000_000);
    assert!(id < later);
    assert_eq!(
        unix_millis_of(&generator.watermark().unwrap()),
        Some(3_000_000)
    );
    // The floor only goes up, and the IDs never go backwards.
    generator.raise_floor(10);
    assert!(generator.new_at(1) > later);
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
