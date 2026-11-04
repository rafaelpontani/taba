use super::*;

/// UC: UC-GTW-004
#[test]
fn the_reserved_characters_are_escaped() {
    assert_eq!(escape("a<b>&\"c'd"), "a&lt;b&gt;&amp;&quot;c&apos;d");
}

/// UC: UC-GTW-004
#[test]
fn control_characters_xml_cannot_carry_are_dropped_and_text_is_kept() {
    assert_eq!(escape("a\u{1}b\u{0}c"), "abc");
    assert_eq!(escape("line\nbreak\ttab café €"), "line\nbreak\ttab café €");
}

/// UC: UC-GTW-004
#[test]
fn elements_are_written_in_order() {
    let mut xml = Xml::new();
    xml.open("Response", Some("urn:x"))
        .leaf("A", "1 < 2")
        .open("B", None)
        .leaf("C", "")
        .close("B")
        .close("Response");
    assert_eq!(
        xml.finish(),
        "<?xml version=\"1.0\"?><Response xmlns=\"urn:x\"><A>1 &lt; 2</A><B><C></C></B></Response>"
    );
}
