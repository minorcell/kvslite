//! WAL (Write-Ahead Log) 文件操作
//!
//! 本模块负责 WAL 文件的读写、追加和恢复操作。
//!
//! ## 职责
//!
//! - **追加写入**：将新记录追加到 WAL 文件末尾
//! - **随机读取**：根据 offset/length 读取 value
//! - **Replay**：启动时重放 WAL 重建索引
//! - **Truncate**：截断损坏的 WAL 尾部
//!
//! ## 文件结构
//!
//! WAL 文件是一系列连续的 Record：
//!
//! ```text
//! | Record 1 | Record 2 | Record 3 | ... | (可能损坏的 Record) |
//! ```
//!
//! ## 崩溃恢复
//!
//! 启动时，Wal::open() 会自动执行 replay：
//!
//! 1. 顺序读取所有记录
//! 2. 验证每条记录的完整性（CRC）
//! 3. 如果遇到损坏记录：
//!    - 截断到最后一条完整记录
//!    - 记录警告信息
//! 4. 返回所有有效的记录

use crate::codec::Record;
use crate::error::Result;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// WAL 文件名
const WAL_FILENAME: &str = "wal.log";

/// WAL 文件管理器
///
/// 负责 WAL 文件的所有 I/O 操作
pub struct Wal {
    /// WAL 文件路径
    #[allow(dead_code)]
    path: PathBuf,
    /// WAL 文件句柄（用于追加写入）
    write_file: File,
    /// WAL 文件句柄（用于随机读取）
    read_file: File,
    /// 当前文件写入位置（字节偏移量）
    offset: u64,
}

/// Replay 统计信息
///
/// 记录 WAL 恢复过程的详细信息，便于调试和监控
#[derive(Debug, Clone, Default)]
pub struct ReplayStats {
    /// 总共扫描的记录数
    pub total_records: usize,
    /// 有效记录数
    pub valid_records: usize,
    /// 损坏的记录数
    pub corrupted_records: usize,
    /// 截断的字节数（0 表示未截断）
    pub truncated_bytes: u64,
}

impl Wal {
    /// 打开或创建 WAL 文件
    ///
    /// ## 参数
    ///
    /// - `dir`: 数据库目录路径
    ///
    /// ## 返回值
    ///
    /// - `Ok((Wal, Vec<Record>, ReplayStats))`: WAL 实例、恢复的记录列表、统计信息
    /// - `Err(Error)`: 如果文件操作失败
    ///
    /// ## 行为
    ///
    /// 1. 如果文件不存在，创建新文件
    /// 2. 如果文件存在，执行 replay 恢复所有有效记录
    /// 3. 如果 replay 发现损坏，自动截断并记录统计信息
    ///
    /// ## 示例
    ///
    /// ```ignore
    /// // 内部 API，通过 Db::open() 间接调用
    /// use kvslite::wal::Wal;
    ///
    /// let (wal, records, stats) = Wal::open("data/db1").unwrap();
    /// println!("Recovered {} records", stats.valid_records);
    /// if stats.truncated_bytes > 0 {
    ///     println!("Warning: truncated {} bytes", stats.truncated_bytes);
    /// }
    /// ```
    pub fn open<P: AsRef<Path>>(dir: P) -> Result<(Self, Vec<Record>, ReplayStats)> {
        // 确保目录存在
        std::fs::create_dir_all(&dir)?;

        let path = dir.as_ref().join(WAL_FILENAME);

        // 先尝试读取现有文件进行 replay
        let (records, stats) = if path.exists() {
            Self::replay(&path)?
        } else {
            (Vec::new(), ReplayStats::default())
        };

        // 打开文件用于追加写入
        let write_file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&path)?;

        // 打开文件用于随机读取
        let read_file = OpenOptions::new()
            .read(true)
            .open(&path)?;

        // 获取当前文件大小（即追加位置）
        let offset = write_file.metadata()?.len();

        let wal = Wal {
            path,
            write_file,
            read_file,
            offset,
        };

        Ok((wal, records, stats))
    }

    /// Replay WAL 文件
    ///
    /// 读取并验证 WAL 中的所有记录。
    ///
    /// ## 错误处理策略
    ///
    /// - 遇到第一个损坏记录时停止
    /// - 截断到最后一条完整记录的末尾
    /// - 返回所有有效记录
    ///
    /// 这种策略保证了：
    /// - 不丢失任何完整写入的数据
    /// - 损坏的部分（未完成的写入）被安全丢弃
    fn replay(path: &Path) -> Result<(Vec<Record>, ReplayStats)> {
        let mut stats = ReplayStats::default();
        let mut records = Vec::new();

        let file = File::open(path)?;
        let file_len = file.metadata()?.len();
        let mut reader = BufReader::new(file);

        let mut last_valid_offset = 0u64;

        loop {
            // 记录当前位置（用于截断）
            let _current_offset = reader.stream_position()?;

            match Record::decode(&mut reader) {
                Ok(Some(record)) => {
                    // 成功解码一条记录
                    stats.total_records += 1;
                    stats.valid_records += 1;
                    records.push(record);

                    // 更新最后一条有效记录的末尾位置
                    last_valid_offset = reader.stream_position()?;
                }
                Ok(None) => {
                    // 正常到达文件末尾
                    break;
                }
                Err(_e) => {
                    // 遇到损坏记录
                    stats.total_records += 1;
                    stats.corrupted_records += 1;

                    // 计算需要截断的字节数
                    stats.truncated_bytes = file_len - last_valid_offset;

                    // 截断文件到最后一条有效记录
                    if stats.truncated_bytes > 0 {
                        drop(reader); // 关闭读取句柄
                        let file = OpenOptions::new().write(true).open(path)?;
                        file.set_len(last_valid_offset)?;
                    }

                    break;
                }
            }
        }

        Ok((records, stats))
    }

    /// 追加一条记录到 WAL
    ///
    /// ## 参数
    ///
    /// - `record`: 要写入的记录
    /// - `sync`: 是否立即 fsync（保证持久化）
    ///
    /// ## 返回值
    ///
    /// - `Ok(u64)`: 写入成功，返回记录在文件中的起始偏移量
    /// - `Err(Error)`: 如果写入失败
    ///
    /// ## 写入流程
    ///
    /// 1. 编码记录为字节
    /// 2. 写入文件
    /// 3. flush 到 OS 缓冲区
    /// 4. 如果 sync=true，调用 fsync 刷到磁盘
    /// 5. 更新内部 offset
    ///
    /// ## 崩溃安全性
    ///
    /// - 如果 sync=true，函数返回 Ok 表示数据已安全落盘
    /// - 如果 sync=false，数据在 OS 缓冲区，崩溃可能丢失
    pub fn append(&mut self, record: &Record, sync: bool) -> Result<u64> {
        // 1. 编码记录
        let data = record.encode()?;

        // 2. 记录起始位置
        let start_offset = self.offset;

        // 3. 写入数据
        self.write_file.write_all(&data)?;

        // 4. Flush 到 OS 缓冲区
        self.write_file.flush()?;

        // 5. 可选：fsync 到磁盘
        if sync {
            self.write_file.sync_data()?;
        }

        // 6. 更新 offset
        self.offset += data.len() as u64;

        Ok(start_offset)
    }

    /// 从指定位置读取数据
    ///
    /// ## 参数
    ///
    /// - `offset`: 起始偏移量（字节）
    /// - `len`: 读取长度（字节）
    ///
    /// ## 返回值
    ///
    /// - `Ok(Vec<u8>)`: 读取的数据
    /// - `Err(Error)`: 如果读取失败
    ///
    /// ## 使用场景
    ///
    /// 内存索引记录了每个 key 对应 value 的位置（offset + len），
    /// 读取时直接调用这个方法获取 value。
    ///
    /// ## 注意
    ///
    /// 这是一个随机 I/O 操作，性能取决于磁盘类型：
    /// - HDD: ~10ms/次
    /// - SSD: ~0.1ms/次
    pub fn read_at(&mut self, offset: u64, len: usize) -> Result<Vec<u8>> {
        // 1. Seek 到目标位置
        self.read_file.seek(SeekFrom::Start(offset))?;

        // 2. 读取数据
        let mut buf = vec![0u8; len];
        std::io::Read::read_exact(&mut self.read_file, &mut buf)?;

        Ok(buf)
    }

    /// 获取当前 WAL 文件大小
    pub fn size(&self) -> u64 {
        self.offset
    }

    /// 获取 WAL 文件路径
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::RecordKind;
    use tempfile::TempDir;

    #[test]
    fn test_create_new_wal() {
        let dir = TempDir::new().unwrap();
        let (wal, records, stats) = Wal::open(dir.path()).unwrap();

        assert_eq!(records.len(), 0);
        assert_eq!(stats.valid_records, 0);
        assert_eq!(wal.size(), 0);
    }

    #[test]
    fn test_append_and_replay() {
        let dir = TempDir::new().unwrap();

        // 写入几条记录
        {
            let (mut wal, _, _) = Wal::open(dir.path()).unwrap();

            let r1 = Record::put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
            let r2 = Record::put(b"key2".to_vec(), b"value2".to_vec()).unwrap();
            let r3 = Record::delete(b"key1".to_vec()).unwrap();

            wal.append(&r1, true).unwrap();
            wal.append(&r2, true).unwrap();
            wal.append(&r3, true).unwrap();
        }

        // 重新打开，验证 replay
        {
            let (_, records, stats) = Wal::open(dir.path()).unwrap();

            assert_eq!(records.len(), 3);
            assert_eq!(stats.valid_records, 3);
            assert_eq!(stats.corrupted_records, 0);
            assert_eq!(stats.truncated_bytes, 0);

            assert_eq!(records[0].key, b"key1");
            assert_eq!(records[0].value, b"value1");
            assert_eq!(records[1].key, b"key2");
            assert_eq!(records[2].kind, RecordKind::Delete);
        }
    }

    #[test]
    fn test_read_at() {
        let dir = TempDir::new().unwrap();
        let (mut wal, _, _) = Wal::open(dir.path()).unwrap();

        // 写入两条记录
        let r1 = Record::put(b"k1".to_vec(), b"v1".to_vec()).unwrap();
        let r2 = Record::put(b"k2".to_vec(), b"v2value2".to_vec()).unwrap();

        let offset1 = wal.append(&r1, true).unwrap();
        let offset2 = wal.append(&r2, true).unwrap();

        // 读取第一条记录的完整数据
        let r1_encoded = r1.encode().unwrap();
        let data1 = wal.read_at(offset1, r1_encoded.len()).unwrap();
        assert!(data1.starts_with(b"KVSL")); // magic

        // 读取第二条记录的完整数据
        let r2_encoded = r2.encode().unwrap();
        let data2 = wal.read_at(offset2, r2_encoded.len()).unwrap();
        assert!(data2.starts_with(b"KVSL"));
    }

    #[test]
    fn test_replay_with_corruption() {
        let dir = TempDir::new().unwrap();
        let wal_path = dir.path().join(WAL_FILENAME);

        // 写入两条完整记录
        {
            let (mut wal, _, _) = Wal::open(dir.path()).unwrap();
            let r1 = Record::put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
            let r2 = Record::put(b"key2".to_vec(), b"value2".to_vec()).unwrap();
            wal.append(&r1, true).unwrap();
            wal.append(&r2, true).unwrap();
        }

        // 手动追加损坏数据（半写入）
        {
            let mut file = OpenOptions::new()
                .append(true)
                .open(&wal_path)
                .unwrap();
            file.write_all(b"KVSL garbage data").unwrap();
        }

        // 重新打开，应该自动截断损坏部分
        {
            let (_, records, stats) = Wal::open(dir.path()).unwrap();

            assert_eq!(records.len(), 2);
            assert_eq!(stats.valid_records, 2);
            assert_eq!(stats.corrupted_records, 1);
            assert!(stats.truncated_bytes > 0);
        }

        // 验证文件已被截断
        let file_len = std::fs::metadata(&wal_path).unwrap().len();
        // 两条记录的实际大小取决于编码
        // 暂时只验证记录被正确恢复
        assert!(file_len > 0);
        assert!(file_len < 100); // 应该小于100字节（两条小记录）
    }
}
