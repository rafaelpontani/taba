use super::*;

/// UC: UC-FAL-007
#[test]
fn the_quorum_is_the_ceiling_of_the_percentage_of_the_healthy_nodes() {
    // 51%: 2 of 3, 3 of 4, 3 of 5 (doc 04, section 6.3).
    assert_eq!(required_votes(3, 51), 2);
    assert_eq!(required_votes(4, 51), 3);
    assert_eq!(required_votes(5, 51), 3);
    assert_eq!(required_votes(2, 51), 2);
    assert_eq!(required_votes(1, 51), 1);
    // Other percentages.
    assert_eq!(required_votes(4, 50), 2);
    assert_eq!(required_votes(5, 100), 5);
    assert_eq!(required_votes(10, 51), 6);
}
