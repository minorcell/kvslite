/// 集成测试
///
/// 测试 kvslite 的完整功能，包括：
/// - 基本操作（put/get/delete）
/// - 崩溃恢复
/// - 边界条件

use kvslite::{Db, Options};
use tempfile::TempDir;

#[test]
fn test_basic_operations() {
    let dir = TempDir::new().unwrap();
    let mut db = Db::open(dir.path(), Options::default()).unwrap();

    // PUT
    db.put(b"hello", b"world").unwrap();
    db.put(b"foo", b"bar").unwrap();

    // GET
    assert_eq!(db.get(b"hello").unwrap().as_deref(), Some(b"world" as &[u8]));
    assert_eq!(db.get(b"foo").unwrap().as_deref(), Some(b"bar" as &[u8]));
    assert_eq!(db.get(b"nonexistent").unwrap(), None);

    // DELETE
    db.delete(b"hello").unwrap();
    assert_eq!(db.get(b"hello").unwrap(), None);
}

#[test]
fn test_last_write_wins() {
    let dir = TempDir::new().unwrap();
    let mut db = Db::open(dir.path(), Options::default()).unwrap();

    db.put(b"key", b"value1").unwrap();
    db.put(b"key", b"value2").unwrap();
    db.put(b"key", b"value3").unwrap();

    assert_eq!(db.get(b"key").unwrap().as_deref(), Some(b"value3" as &[u8]));
}

#[test]
fn test_persistence_across_reopen() {
    let dir = TempDir::new().unwrap();

    // 第一次打开，写入数据
    {
        let mut db = Db::open(dir.path(), Options::default()).unwrap();
        db.put(b"persistent", b"data").unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
    }

    // 第二次打开，验证数据还在
    {
        let mut db = Db::open(dir.path(), Options::default()).unwrap();
        assert_eq!(
            db.get(b"persistent").unwrap().as_deref(),
            Some(b"data" as &[u8])
        );
        assert_eq!(db.get(b"key1").unwrap().as_deref(), Some(b"value1" as &[u8]));
        assert_eq!(db.get(b"key2").unwrap().as_deref(), Some(b"value2" as &[u8]));
    }
}

#[test]
fn test_delete_and_reopen() {
    let dir = TempDir::new().unwrap();

    {
        let mut db = Db::open(dir.path(), Options::default()).unwrap();
        db.put(b"temp", b"value").unwrap();
        db.delete(b"temp").unwrap();
    }

    {
        let mut db = Db::open(dir.path(), Options::default()).unwrap();
        assert_eq!(db.get(b"temp").unwrap(), None);
    }
}

#[test]
fn test_empty_key() {
    let dir = TempDir::new().unwrap();
    let mut db = Db::open(dir.path(), Options::default()).unwrap();

    // 空 key 是允许的
    db.put(b"", b"empty_key_value").unwrap();
    assert_eq!(
        db.get(b"").unwrap().as_deref(),
        Some(b"empty_key_value" as &[u8])
    );
}

#[test]
fn test_empty_value() {
    let dir = TempDir::new().unwrap();
    let mut db = Db::open(dir.path(), Options::default()).unwrap();

    // 空 value 是允许的
    db.put(b"empty_value", b"").unwrap();
    assert_eq!(db.get(b"empty_value").unwrap().as_deref(), Some(b"" as &[u8]));
}

#[test]
fn test_many_keys() {
    let dir = TempDir::new().unwrap();
    let mut db = Db::open(dir.path(), Options::default()).unwrap();

    // 写入 1000 个 key
    for i in 0..1000 {
        let key = format!("key_{}", i);
        let value = format!("value_{}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // 验证所有 key
    for i in 0..1000 {
        let key = format!("key_{}", i);
        let expected_value = format!("value_{}", i);
        assert_eq!(
            db.get(key.as_bytes()).unwrap().as_deref(),
            Some(expected_value.as_bytes())
        );
    }

    // 验证统计信息
    let stats = db.stats();
    assert_eq!(stats.key_count, 1000);
}

#[test]
fn test_binary_data() {
    let dir = TempDir::new().unwrap();
    let mut db = Db::open(dir.path(), Options::default()).unwrap();

    // 测试二进制数据（包含 null 字节）
    let binary_key = b"\x00\x01\x02\xFF";
    let binary_value = b"\xDE\xAD\xBE\xEF";

    db.put(binary_key, binary_value).unwrap();
    assert_eq!(db.get(binary_key).unwrap().as_deref(), Some(binary_value as &[u8]));
}

#[test]
fn test_no_sync_mode() {
    let dir = TempDir::new().unwrap();
    let opts = Options {
        sync_on_write: false,
    };
    let mut db = Db::open(dir.path(), opts).unwrap();

    db.put(b"fast_key", b"fast_value").unwrap();
    assert_eq!(
        db.get(b"fast_key").unwrap().as_deref(),
        Some(b"fast_value" as &[u8])
    );
}

#[test]
fn test_stats() {
    let dir = TempDir::new().unwrap();
    let mut db = Db::open(dir.path(), Options::default()).unwrap();

    let initial_stats = db.stats();
    assert_eq!(initial_stats.key_count, 0);

    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();

    let stats = db.stats();
    assert_eq!(stats.key_count, 2);
    assert!(stats.wal_size > 0);

    db.delete(b"key1").unwrap();

    let stats = db.stats();
    assert_eq!(stats.key_count, 1);
}
