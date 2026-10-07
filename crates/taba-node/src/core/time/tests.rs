use super::*;

/// UC: UC-CNV-001
#[test]
fn formats_as_rfc3339_utc_with_milliseconds() {
    assert_eq!(format_ms(1_791_288_000_000), "2026-10-06T12:00:00.000Z");
    assert_eq!(format_ms(1_791_288_000_007), "2026-10-06T12:00:00.007Z");
    assert_eq!(format_ms(0), "1970-01-01T00:00:00.000Z");
}

#[test]
fn parse_is_the_inverse_of_format() {
    for ms in [0, 1, 1_791_288_000_123, 4_102_444_800_999] {
        assert_eq!(parse_ms(&format_ms(ms)), Some(ms));
    }
}

#[test]
fn parse_rejects_other_shapes() {
    for bad in [
        "",
        "2026-10-06",
        "2026-10-06T12:00:00Z",
        "2026-10-06T12:00:00.000+01:00",
        "ontem",
    ] {
        assert_eq!(parse_ms(bad), None, "{bad:?}");
    }
}
