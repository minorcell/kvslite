use kvslite::KvStore;
use tempfile::tempdir;

#[test]
fn set_and_read_back() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("store.json");

    {
        let mut store = KvStore::open(&file).unwrap();
        store.set("token", "abc123").unwrap();
        store.set("theme", "dark").unwrap();
        assert_eq!(store.get("token"), Some("abc123"));
    }

    // New instance should see persisted values.
    let store = KvStore::open(&file).unwrap();
    assert_eq!(store.get("token"), Some("abc123"));
    assert_eq!(store.get("theme"), Some("dark"));
}

#[test]
fn remove_and_clear() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("store.json");

    let mut store = KvStore::open(&file).unwrap();
    store.set("k1", "v1").unwrap();
    store.set("k2", "v2").unwrap();

    store.remove("k1").unwrap();
    assert_eq!(store.get("k1"), None);
    assert_eq!(store.get("k2"), Some("v2"));

    store.clear().unwrap();
    assert_eq!(store.get("k2"), None);
    assert!(store.get("anything").is_none());
}
