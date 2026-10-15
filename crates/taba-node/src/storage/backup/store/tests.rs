use std::sync::Arc;

use super::*;

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

/// UC: UC-BKP-003
#[test]
fn the_memory_store_does_what_the_object_store_trait_promises() {
    block_on(async {
        let store = MemoryStore::new();
        let etag = store.put("a/1", b"0123456789".to_vec()).await.unwrap();
        assert_eq!(store.get("a/1").await.unwrap().unwrap().etag, etag);
        assert_eq!(
            store.get_range("a/1", 2, 3).await.unwrap().unwrap(),
            b"234",
            "Range inside"
        );
        assert_eq!(
            store.get_range("a/1", 8, 100).await.unwrap().unwrap(),
            b"89",
            "the end past the size is no error"
        );
        assert_eq!(store.get_suffix("a/1", 4).await.unwrap().unwrap(), b"6789");
        assert!(store.get("a/2").await.unwrap().is_none());
        assert!(store.get_range("a/2", 0, 1).await.unwrap().is_none());

        store.put("a/2", b"x".to_vec()).await.unwrap();
        store.put("b/1", b"y".to_vec()).await.unwrap();
        let listed: Vec<_> = store
            .list("a/")
            .await
            .unwrap()
            .into_iter()
            .map(|o| o.key)
            .collect();
        assert_eq!(listed, ["a/1", "a/2"]);

        store.delete("a/1").await.unwrap();
        store.delete("a/1").await.unwrap();
        assert!(store.get("a/1").await.unwrap().is_none());
    });
}

/// UC: UC-BKP-003
#[test]
fn conditional_writes_create_once_and_replace_only_the_known_version() {
    block_on(async {
        let store = MemoryStore::new();
        assert!(store.put_if_absent("lock", b"1".to_vec()).await.unwrap());
        assert!(!store.put_if_absent("lock", b"2".to_vec()).await.unwrap());
        let etag = store.get("lock").await.unwrap().unwrap().etag;
        assert!(
            !store
                .put_if_match("lock", b"3".to_vec(), "other")
                .await
                .unwrap()
        );
        assert!(
            store
                .put_if_match("lock", b"3".to_vec(), &etag)
                .await
                .unwrap()
        );
        assert_eq!(store.peek("lock").unwrap(), b"3");
        assert!(
            !store
                .put_if_match("sumiu", b"x".to_vec(), &etag)
                .await
                .unwrap()
        );
    });
}

/// UC: UC-BKP-003, UC-BKP-025
#[test]
fn a_store_without_conditional_writes_says_so_and_one_that_is_down_fails() {
    block_on(async {
        let store = MemoryStore::new();
        store.set_conditional(false);
        assert!(matches!(
            store.put_if_absent("k", vec![]).await,
            Err(StoreError::Unsupported(_))
        ));
        store.set_down(true);
        assert!(matches!(
            store.put("k", vec![]).await,
            Err(StoreError::Unavailable(_))
        ));
        assert!(store.list("").await.is_err());
        store.set_down(false);
        store.put("k", vec![]).await.unwrap();
    });
}

/// UC: UC-BKP-004
#[test]
fn the_base_prefix_goes_in_front_of_every_key_and_out_of_the_listing() {
    block_on(async {
        let raw = Arc::new(MemoryStore::new());
        let store = Prefixed::new(raw.clone(), "/taba/prod/");
        store.put("topics/a.json", b"{}".to_vec()).await.unwrap();
        assert_eq!(raw.keys(""), ["taba/prod/topics/a.json"]);
        let listed = store.list("topics/").await.unwrap();
        assert_eq!(listed[0].key, "topics/a.json");
        assert!(store.get("topics/a.json").await.unwrap().is_some());

        let bare = Prefixed::new(raw.clone(), "");
        bare.put("x", vec![1]).await.unwrap();
        assert!(
            raw.peek("x").is_some(),
            "without a prefix, the key stays as it came"
        );
    });
}
