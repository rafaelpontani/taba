use super::*;

#[test]
fn a_popup_is_centered_and_never_larger_than_the_area() {
    let area = Rect::new(0, 0, 80, 24);
    let r = centered(area, 40, 10);
    assert_eq!((r.x, r.y, r.width, r.height), (20, 7, 40, 10));
    let big = centered(area, 200, 100);
    assert_eq!((big.width, big.height), (80, 24));
}

/// TUI: TUI-CLU-03
#[test]
fn a_node_id_keeps_its_start_and_its_end_so_nodes_made_together_differ() {
    assert_eq!(short("01a13640-7774-7000-8000-00000000abcd"), "01a1…00abcd");
    assert_ne!(
        short("01a13640-7774-7000-8000-000000000001"),
        short("01a13640-7774-7000-8000-000000000002")
    );
    assert_eq!(short("ab"), "ab");
    assert_eq!(short("123456789012"), "123456789012");
}

/// TUI: TUI-MON-07
#[test]
fn a_file_tag_is_the_last_eight_letters_and_digits() {
    use crate::tui::model::file_tag;
    assert_eq!(file_tag("01a13640-7774-7000-8000-00000000abcd"), "0000abcd");
    assert_eq!(file_tag("a/b"), "ab");
}
