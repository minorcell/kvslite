//! kvslite 使用示例
//!
//! 展示基本的 CRUD 操作和崩溃恢复

use kvslite::{Db, Options};

fn main() -> kvslite::Result<()> {
    println!("=== kvslite v0.1 示例 ===\n");

    // 1. 打开数据库
    println!("1. 打开数据库...");
    let mut db = Db::open("data/example_db", Options::default())?;
    println!("   ✓ 数据库已打开\n");

    // 2. 写入数据
    println!("2. 写入数据...");
    db.put(b"user:1:name", b"Alice")?;
    db.put(b"user:1:age", b"30")?;
    db.put(b"user:1:email", b"alice@example.com")?;
    println!("   ✓ 已写入 3 个键值对\n");

    // 3. 读取数据
    println!("3. 读取数据...");
    if let Some(name) = db.get(b"user:1:name")? {
        println!("   user:1:name = {}", String::from_utf8_lossy(&name));
    }
    if let Some(age) = db.get(b"user:1:age")? {
        println!("   user:1:age = {}", String::from_utf8_lossy(&age));
    }
    if let Some(email) = db.get(b"user:1:email")? {
        println!("   user:1:email = {}", String::from_utf8_lossy(&email));
    }
    println!();

    // 4. 更新数据
    println!("4. 更新数据...");
    db.put(b"user:1:age", b"31")?;
    if let Some(age) = db.get(b"user:1:age")? {
        println!("   user:1:age (更新后) = {}", String::from_utf8_lossy(&age));
    }
    println!();

    // 5. 删除数据
    println!("5. 删除数据...");
    db.delete(b"user:1:email")?;
    match db.get(b"user:1:email")? {
        Some(_) => println!("   ✗ 删除失败"),
        None => println!("   ✓ user:1:email 已删除"),
    }
    println!();

    // 6. 批量写入
    println!("6. 批量写入 100 个键...");
    for i in 0..100 {
        let key = format!("item:{}", i);
        let value = format!("value_{}", i);
        db.put(key.as_bytes(), value.as_bytes())?;
    }
    println!("   ✓ 批量写入完成\n");

    // 7. 查看统计信息
    println!("7. 数据库统计信息:");
    let stats = db.stats();
    println!("   - 键数量: {}", stats.key_count);
    println!("   - WAL 文件大小: {} 字节", stats.wal_size);
    println!();

    println!("=== 示例完成 ===");
    println!("\n提示：重新运行此程序，数据会持久化（崩溃恢复）");
    println!("可以手动删除 data/example_db 目录来清理数据");

    Ok(())
}
