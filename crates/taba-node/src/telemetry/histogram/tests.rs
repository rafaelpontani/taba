use super::*;

const BOUNDS: &[f64] = &[1.0, 2.0, 5.0];

#[test]
fn counts_land_in_the_first_bucket_that_fits() {
    let mut h = Histogram::new(BOUNDS);
    for v in [0.5, 1.0, 1.5, 4.0, 9.0] {
        h.record(v);
    }
    assert_eq!(h.count(), 5);
    assert_eq!(h.sum(), 16.0);
    assert_eq!(h.cumulative(), [2, 3, 4, 5]);
}

#[test]
fn percentile_is_the_upper_bound_of_the_bucket() {
    let mut h = Histogram::new(BOUNDS);
    assert_eq!(
        h.percentile(0.5),
        None,
        "with no observation there is no percentile"
    );
    for _ in 0..90 {
        h.record(0.5);
    }
    for _ in 0..9 {
        h.record(1.5);
    }
    h.record(4.0);
    assert_eq!(h.percentile(0.50), Some(1.0));
    assert_eq!(h.percentile(0.95), Some(2.0));
    assert_eq!(h.percentile(0.99), Some(2.0));
    assert_eq!(h.percentile(1.0), Some(5.0));
}

#[test]
fn what_passes_the_last_bound_reports_the_last_bound() {
    let mut h = Histogram::new(BOUNDS);
    h.record(100.0);
    assert_eq!(h.percentile(0.99), Some(5.0));
    assert_eq!(h.cumulative(), [0, 0, 0, 1]);
}

#[test]
fn merging_adds_the_counts() {
    let (mut a, mut b) = (Histogram::new(BOUNDS), Histogram::new(BOUNDS));
    a.record(0.5);
    b.record(3.0);
    b.record(3.0);
    a.merge(&b);
    assert_eq!(a.count(), 3);
    assert_eq!(a.cumulative(), [1, 1, 3, 3]);
}
