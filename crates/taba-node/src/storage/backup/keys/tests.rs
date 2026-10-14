use super::*;

/// UC: UC-BKP-005
#[test]
fn the_key_layout_follows_the_documented_one() {
    assert_eq!(topic("pedidos"), "topics/pedidos.json");
    assert_eq!(token("t-1"), "tokens/t-1.json");
    assert_eq!(node("n-1"), "nodes/n-1.json");
    assert_eq!(node_id_of("nodes/n-1.json"), Some("n-1"));
    assert_eq!(node_id_of("tokens/n-1.json"), None);
    let seg = segment("pedidos", "tid-1", "0198-aaaa", "node-9", 7);
    assert_eq!(seg, "messages/pedidos/tid-1/0198-aaaa_node-9_000007.seg");
    assert_eq!(first_id_of(&seg), Some("0198-aaaa"));
    assert_eq!(writer_of(&seg), Some("node-9"));
    assert_eq!(topic_of(&seg), Some(("pedidos", "tid-1")));
    let jrn = journal("pedidos", "tid-1", 42, "node-9", 8);
    assert_eq!(
        jrn,
        "journal/pedidos/tid-1/0000000000000042_node-9_000008.jrn"
    );
    assert_eq!(topic_of(&jrn), Some(("pedidos", "tid-1")));
}

/// UC: UC-BKP-005
#[test]
fn segments_of_a_recreated_topic_do_not_share_a_prefix_with_the_old_one() {
    assert_ne!(
        segments_prefix("pedidos", "velho"),
        segments_prefix("pedidos", "novo")
    );
    assert!(segments_prefix("pedidos", "velho").starts_with(&topic_messages_prefix("pedidos")));
    assert!(journals_prefix("pedidos", "velho").starts_with(&topic_journal_prefix("pedidos")));
}
