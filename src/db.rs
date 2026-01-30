//! 数据库主 API
//!
//! 本模块提供 kvslite 的对外接口，组合 WAL 和内存索引实现完整的 KV 存储。
//!
//! ## 架构
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │              Db (pub API)            │
//! ├──────────────────────────────────────┤
//! │  - put(key, value)                   │
//! │  - get(key) -> Option<value>         │
//! │  - delete(key)                       │
//! └────────────┬────────────┬────────────┘
//!              │            │
//!      ┌───────▼──────┐  ┌──▼──────────┐
//!      │     WAL      │  │    Index    │
//!      │  (wal.log)   │  │ (HashMap)   │
//!      └──────────────┘  └─────────────┘
//! ```
//!
//! ## 数据流
//!
//! ### 写入 (PUT)
//!
//! ```text
//! 1. 验证 key/value 大小
//! 2. 编码 Record
//! 3. 追加到 WAL ──┐
//! 4. fsync (可选)  │
//! 5. 更新索引 ─────┘ (只有持久化成功后才更新)
//! ```
//!
//! ### 读取 (GET)
//!
//! ```text
//! 1. 查询索引 ──┐
//! 2. 如果找到   │
//!    ├─ 从 WAL 读取 value (随机 I/O)
//!    └─ 返回 value
//! 3. 如果未找到
//!    └─ 返回 None
//! ```
//!
//! ### 删除 (DELETE)
//!
//! ```text
//! 1. 写入 DELETE 记录到 WAL
//! 2. 从索引中移除 key
//! ```
//!
//! ## 内存索引
//!
//! 索引记录每个 key 对应 value 在 WAL 文件中的位置：
//!
//! ```text
//! HashMap<Vec<u8>, ValuePos>
//!
//! ValuePos {
//!     offset: u64,  // value 在 WAL 中的字节偏移量
//!     len: usize,   // value 的字节长度
//! }
//! ```
//!
//! ### 为什么不缓存 value？
//!
//! v0.1 的设计选择是"索引在内存，value 在磁盘"：
//!
//! **优点：**
//! - 内存占用可控（只存储 key + 位置）
//! - 支持大 value（最大 1MB）
//!
//! **缺点：**
//! - 每次读取都需要磁盘 I/O
//!
//! 未来版本可以增加 LRU 缓存来优化热点数据读取。

use crate::codec::{Record, RecordKind};
use crate::error::Result;
use crate::wal::{ReplayStats, Wal};
use std::collections::HashMap;
use std::path::Path;

/// Value 在 WAL 文件中的位置信息
#[derive(Debug, Clone, Copy)]
struct ValuePos {
    /// value 的起始偏移量（字节）
    offset: u64,
    /// value 的长度（字节）
    len: usize,
}

/// 数据库配置选项
#[derive(Debug, Clone)]
pub struct Options {
    /// 是否在每次写入后同步到磁盘
    ///
    /// - `true`: 每次 put/delete 都调用 fsync，保证数据持久化
    ///   - 优点：崩溃后不丢数据
    ///   - 缺点：写入性能较差（~1ms/次）
    ///
    /// - `false`: 只 flush 到 OS 缓冲区，不调用 fsync
    ///   - 优点：写入性能好（~0.01ms/次）
    ///   - 缺点：崩溃可能丢失最后一小段写入
    ///
    /// 默认：`true`（安全优先）
    pub sync_on_write: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            sync_on_write: true,
        }
    }
}

/// kvslite 数据库实例
///
/// ## 线程安全性
///
/// `Db` 不是线程安全的（没有实现 `Sync`）。
///
/// v0.1 设计为单线程使用：
/// - 写操作需要 `&mut self`
/// - 读操作需要 `&mut self`（因为需要 seek 文件）
///
/// 如果需要多线程访问，可以：
/// - 用 `Arc<Mutex<Db>>` 包装
/// - 等待 v0.6 的并发支持
pub struct Db {
    /// WAL 管理器
    wal: Wal,
    /// 内存索引：key -> value 位置
    index: HashMap<Vec<u8>, ValuePos>,
    /// 配置选项
    opts: Options,
}

impl Db {
    /// 打开或创建数据库
    ///
    /// ## 参数
    ///
    /// - `path`: 数据库目录路径（不存在会自动创建）
    /// - `opts`: 配置选项
    ///
    /// ## 返回值
    ///
    /// - `Ok(Db)`: 数据库实例
    /// - `Err(Error)`: 如果打开失败
    ///
    /// ## 行为
    ///
    /// 1. 创建数据库目录（如果不存在）
    /// 2. 打开 WAL 文件
    /// 3. 如果 WAL 文件已存在，执行 replay 恢复数据
    /// 4. 重建内存索引
    ///
    /// ## 崩溃恢复
    ///
    /// 如果上次运行时发生崩溃，open() 会自动恢复：
    /// - 扫描 WAL 文件
    /// - 验证每条记录的完整性
    /// - 如果发现损坏，截断到最后一条完整记录
    ///
    /// ## 示例
    ///
    /// ```no_run
    /// use kvslite::{Db, Options};
    ///
    /// // 使用默认配置
    /// let db = Db::open("data/db1", Options::default()).unwrap();
    ///
    /// // 自定义配置
    /// let opts = Options {
    ///     sync_on_write: false,  // 性能优先
    /// };
    /// let db = Db::open("data/db2", opts).unwrap();
    /// ```
    pub fn open<P: AsRef<Path>>(path: P, opts: Options) -> Result<Self> {
        // 1. 打开 WAL 并 replay
        let (wal, records, stats) = Wal::open(path)?;

        // 2. 如果发生了截断，打印警告
        if stats.truncated_bytes > 0 {
            eprintln!(
                "Warning: WAL recovery truncated {} bytes ({} corrupted records)",
                stats.truncated_bytes, stats.corrupted_records
            );
        }

        // 3. 重建内存索引
        let index = Self::rebuild_index(&records, &stats);

        Ok(Db { wal, index, opts })
    }

    /// 从 replay 的记录重建内存索引
    ///
    /// ## 逻辑
    ///
    /// 顺序扫描所有记录：
    /// - 遇到 PUT：更新索引（last-write-wins）
    /// - 遇到 DELETE：从索引中移除
    ///
    /// ## 注意
    ///
    /// 这里我们需要知道每个 record 在文件中的具体位置，
    /// 但 replay 返回的只是 Record 对象。
    ///
    /// 为了计算位置，我们需要重新编码每条 record 来获得其大小。
    ///
    /// ## 优化方向（未来版本）
    ///
    /// - Replay 时直接返回 (Record, offset, len)
    /// - 避免重复编码
    fn rebuild_index(records: &[Record], _stats: &ReplayStats) -> HashMap<Vec<u8>, ValuePos> {
        let mut index = HashMap::new();
        let mut offset = 0u64;

        for record in records {
            // 计算这条记录的大小（需要重新编码）
            // 这不是最优的，但 v0.1 优先正确性
            let encoded = record.encode().unwrap();
            let record_len = encoded.len() as u64;

            match record.kind {
                RecordKind::Put => {
                    // 计算 value 在文件中的位置
                    // value 位于 record 的末尾（crc 之前）
                    let value_offset_in_record = record_len - 4 - record.value.len() as u64;
                    let value_pos = ValuePos {
                        offset: offset + value_offset_in_record,
                        len: record.value.len(),
                    };

                    index.insert(record.key.clone(), value_pos);
                }
                RecordKind::Delete => {
                    // 从索引中移除
                    index.remove(&record.key);
                }
            }

            offset += record_len;
        }

        index
    }

    /// 写入键值对
    ///
    /// ## 参数
    ///
    /// - `key`: 键（最大 1KB）
    /// - `value`: 值（最大 1MB）
    ///
    /// ## 返回值
    ///
    /// - `Ok(())`: 写入成功
    /// - `Err(Error)`: 如果写入失败或超出大小限制
    ///
    /// ## 行为
    ///
    /// 1. 验证 key/value 大小
    /// 2. 创建 PUT 记录
    /// 3. 追加到 WAL 文件
    /// 4. 如果 sync_on_write=true，调用 fsync
    /// 5. 更新内存索引
    ///
    /// ## 语义
    ///
    /// - 如果 key 已存在，覆盖旧值（last-write-wins）
    /// - 函数返回 `Ok` 表示数据已安全持久化（如果 sync_on_write=true）
    ///
    /// ## 示例
    ///
    /// ```no_run
    /// use kvslite::{Db, Options};
    ///
    /// let mut db = Db::open("data/db1", Options::default()).unwrap();
    /// db.put(b"user:1:name", b"Alice").unwrap();
    /// db.put(b"user:1:age", b"30").unwrap();
    /// ```
    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 创建 PUT 记录（会验证大小）
        let record = Record::put(key.to_vec(), value.to_vec())?;

        // 2. 追加到 WAL
        let record_offset = self.wal.append(&record, self.opts.sync_on_write)?;

        // 3. 计算 value 在文件中的位置
        // value 在 record 的末尾（crc 之前）
        let encoded = record.encode()?; // TODO: 优化，避免重复编码
        let record_len = encoded.len() as u64;
        let value_offset_in_record = record_len - 4 - value.len() as u64;
        let value_offset = record_offset + value_offset_in_record;

        // 4. 更新索引
        self.index.insert(
            key.to_vec(),
            ValuePos {
                offset: value_offset,
                len: value.len(),
            },
        );

        Ok(())
    }

    /// 读取键对应的值
    ///
    /// ## 参数
    ///
    /// - `key`: 要查询的键
    ///
    /// ## 返回值
    ///
    /// - `Ok(Some(Vec<u8>))`: 找到 key，返回 value
    /// - `Ok(None)`: key 不存在
    /// - `Err(Error)`: 如果读取失败
    ///
    /// ## 示例
    ///
    /// ```no_run
    /// use kvslite::{Db, Options};
    ///
    /// let mut db = Db::open("data/db1", Options::default()).unwrap();
    /// db.put(b"key", b"value").unwrap();
    ///
    /// let value = db.get(b"key").unwrap();
    /// assert_eq!(value.as_deref(), Some(b"value" as &[u8]));
    ///
    /// let missing = db.get(b"nonexistent").unwrap();
    /// assert_eq!(missing, None);
    /// ```
    pub fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 在索引中查找
        match self.index.get(key) {
            Some(pos) => {
                // 2. 从 WAL 读取 value
                let value = self.wal.read_at(pos.offset, pos.len)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// 删除键
    ///
    /// ## 参数
    ///
    /// - `key`: 要删除的键
    ///
    /// ## 返回值
    ///
    /// - `Ok(())`: 删除成功（无论 key 是否存在）
    /// - `Err(Error)`: 如果操作失败
    ///
    /// ## 行为
    ///
    /// 1. 写入 DELETE 记录到 WAL
    /// 2. 从内存索引中移除 key
    ///
    /// ## 注意
    ///
    /// - 删除不会立即释放磁盘空间（WAL 是追加的）
    /// - 需要 Compaction（v0.2）来回收空间
    /// - 删除不存在的 key 也会写入 WAL（保证操作的持久化语义）
    ///
    /// ## 示例
    ///
    /// ```no_run
    /// use kvslite::{Db, Options};
    ///
    /// let mut db = Db::open("data/db1", Options::default()).unwrap();
    /// db.put(b"key", b"value").unwrap();
    /// db.delete(b"key").unwrap();
    ///
    /// assert_eq!(db.get(b"key").unwrap(), None);
    /// ```
    pub fn delete(&mut self, key: &[u8]) -> Result<()> {
        // 1. 创建 DELETE 记录
        let record = Record::delete(key.to_vec())?;

        // 2. 追加到 WAL
        self.wal.append(&record, self.opts.sync_on_write)?;

        // 3. 从索引中移除
        self.index.remove(key);

        Ok(())
    }

    /// 获取数据库统计信息
    ///
    /// ## 返回值
    ///
    /// 返回一个包含各种统计数据的结构体
    pub fn stats(&self) -> DbStats {
        DbStats {
            key_count: self.index.len(),
            wal_size: self.wal.size(),
        }
    }
}

/// 数据库统计信息
#[derive(Debug, Clone)]
pub struct DbStats {
    /// 当前 key 的数量
    pub key_count: usize,
    /// WAL 文件大小（字节）
    pub wal_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_put_get() {
        let dir = TempDir::new().unwrap();
        let mut db = Db::open(dir.path(), Options::default()).unwrap();

        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();

        assert_eq!(db.get(b"key1").unwrap().as_deref(), Some(b"value1" as &[u8]));
        assert_eq!(db.get(b"key2").unwrap().as_deref(), Some(b"value2" as &[u8]));
        assert_eq!(db.get(b"key3").unwrap(), None);
    }

    #[test]
    fn test_overwrite() {
        let dir = TempDir::new().unwrap();
        let mut db = Db::open(dir.path(), Options::default()).unwrap();

        db.put(b"key", b"value1").unwrap();
        db.put(b"key", b"value2").unwrap();

        assert_eq!(db.get(b"key").unwrap().as_deref(), Some(b"value2" as &[u8]));
    }

    #[test]
    fn test_delete() {
        let dir = TempDir::new().unwrap();
        let mut db = Db::open(dir.path(), Options::default()).unwrap();

        db.put(b"key", b"value").unwrap();
        assert_eq!(db.get(b"key").unwrap().as_deref(), Some(b"value" as &[u8]));

        db.delete(b"key").unwrap();
        assert_eq!(db.get(b"key").unwrap(), None);
    }

    #[test]
    fn test_reopen_persistence() {
        let dir = TempDir::new().unwrap();

        // 写入数据
        {
            let mut db = Db::open(dir.path(), Options::default()).unwrap();
            db.put(b"key1", b"value1").unwrap();
            db.put(b"key2", b"value2").unwrap();
            db.delete(b"key1").unwrap();
        }

        // 重新打开，验证持久化
        {
            let mut db = Db::open(dir.path(), Options::default()).unwrap();
            assert_eq!(db.get(b"key1").unwrap(), None);
            assert_eq!(db.get(b"key2").unwrap().as_deref(), Some(b"value2" as &[u8]));
        }
    }

    #[test]
    fn test_stats() {
        let dir = TempDir::new().unwrap();
        let mut db = Db::open(dir.path(), Options::default()).unwrap();

        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();

        let stats = db.stats();
        assert_eq!(stats.key_count, 2);
        assert!(stats.wal_size > 0);
    }

    #[test]
    fn test_large_value() {
        let dir = TempDir::new().unwrap();
        let mut db = Db::open(dir.path(), Options::default()).unwrap();

        // 1MB value（最大限制）
        let large_value = vec![0xAB; 1024 * 1024];
        db.put(b"large", &large_value).unwrap();

        let retrieved = db.get(b"large").unwrap().unwrap();
        assert_eq!(retrieved, large_value);
    }

    #[test]
    fn test_sync_option() {
        let dir = TempDir::new().unwrap();
        let opts = Options {
            sync_on_write: false, // 不 fsync，更快
        };
        let mut db = Db::open(dir.path(), opts).unwrap();

        db.put(b"key", b"value").unwrap();
        assert_eq!(db.get(b"key").unwrap().as_deref(), Some(b"value" as &[u8]));
    }
}
