//! WAL 记录的编解码
//!
//! 本模块负责 WAL（Write-Ahead Log）记录的二进制序列化和反序列化。
//!
//! ## 记录格式 (v0.1)
//!
//! ```text
//! +-------+--------+---------+------+----------+----------+-----+-------+--------+
//! | magic | rec_len| version | kind | key_len  | val_len  | key | value | crc32  |
//! +-------+--------+---------+------+----------+----------+-----+-------+--------+
//!   4B      4B       1B       1B      4B         4B        var   var     4B
//! ```
//!
//! ### 字段说明
//!
//! - `magic`: 固定值 `KVSL` (0x4B56534C)，用于识别记录边界
//! - `rec_len`: 整个记录的长度（包括 magic 和 crc32），用于快速跳过记录
//! - `version`: 格式版本号，当前为 1
//! - `kind`: 记录类型
//!   - `1` = PUT（写入键值对）
//!   - `2` = DELETE（删除键）
//! - `key_len`: key 的字节长度（little-endian u32）
//! - `val_len`: value 的字节长度（little-endian u32）
//! - `key`: key 的字节内容
//! - `value`: value 的字节内容
//! - `crc32`: CRC32 校验和，覆盖 `rec_len..value` 的所有字节
//!
//! ## 设计要点
//!
//! ### 1. 为什么在开头放 magic？
//!
//! - 快速识别记录边界，便于损坏恢复
//! - 如果 WAL 文件中间损坏，可以扫描后续的 magic 尝试恢复
//!
//! ### 2. 为什么需要 rec_len？
//!
//! - 快速跳过记录（不需要解析 key/value）
//! - 防止恶意或损坏的 key_len/val_len 导致内存耗尽
//! - 可以先验证 rec_len 是否合理（如 < 128MB）
//!
//! ### 3. 为什么 CRC 覆盖 rec_len？
//!
//! - 如果只覆盖 version..value，那么 rec_len 损坏时无法检测
//! - 将 rec_len 纳入校验范围，可以检测所有字段的损坏
//!
//! ### 4. 为什么使用 CRC32 而不是 SHA256？
//!
//! - CRC32 足以检测随机错误（bit flip、截断）
//! - 性能更好（硬件加速），占用空间更小（4 字节）
//! - kvslite 是本地存储，不需要抵御恶意篡改（那是加密的职责）

use crate::error::{Error, Result};
use crc32fast::Hasher;
use std::io::{Read, Write};

/// Magic 字节：KVSL (0x4B56534C)
const MAGIC: [u8; 4] = *b"KVSL";

/// 当前格式版本
const VERSION: u8 = 1;

/// 记录类型：PUT
const KIND_PUT: u8 = 1;

/// 记录类型：DELETE
const KIND_DELETE: u8 = 2;

/// 最大 key 大小：1KB
///
/// 限制原因：
/// - 防止恶意或损坏的数据导致内存耗尽
/// - 鼓励使用短 key（更高效）
const MAX_KEY_SIZE: usize = 1024;

/// 最大 value 大小：1MB
///
/// 限制原因：
/// - kvslite 优化小值存储
/// - 大文件应该存储在文件系统，kvslite 只存元数据
const MAX_VALUE_SIZE: usize = 1024 * 1024;

/// 最大记录大小：2MB（为 header + key + value + crc 留出余量）
const MAX_RECORD_SIZE: usize = 2 * 1024 * 1024;

/// 记录头部大小（不包括 key/value/crc）
///
/// magic(4) + rec_len(4) + version(1) + kind(1) + key_len(4) + val_len(4) = 18 字节
const HEADER_SIZE: usize = 18;

/// WAL 记录
///
/// 表示一次写入操作（PUT 或 DELETE）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// 记录类型
    pub kind: RecordKind,
    /// 键
    pub key: Vec<u8>,
    /// 值（DELETE 时为空）
    pub value: Vec<u8>,
}

/// 记录类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// 写入键值对
    Put,
    /// 删除键
    Delete,
}

impl Record {
    /// 创建一个 PUT 记录
    pub fn put(key: Vec<u8>, value: Vec<u8>) -> Result<Self> {
        // 验证大小限制
        if key.len() > MAX_KEY_SIZE {
            return Err(Error::KeyTooLarge {
                size: key.len(),
                max: MAX_KEY_SIZE,
            });
        }
        if value.len() > MAX_VALUE_SIZE {
            return Err(Error::ValueTooLarge {
                size: value.len(),
                max: MAX_VALUE_SIZE,
            });
        }

        Ok(Record {
            kind: RecordKind::Put,
            key,
            value,
        })
    }

    /// 创建一个 DELETE 记录
    pub fn delete(key: Vec<u8>) -> Result<Self> {
        if key.len() > MAX_KEY_SIZE {
            return Err(Error::KeyTooLarge {
                size: key.len(),
                max: MAX_KEY_SIZE,
            });
        }

        Ok(Record {
            kind: RecordKind::Delete,
            key,
            value: Vec::new(),
        })
    }

    /// 编码记录到字节流
    ///
    /// ## 返回值
    ///
    /// - `Ok(Vec<u8>)`: 编码后的字节数组
    /// - `Err(Error)`: 如果写入失败
    ///
    /// ## 格式
    ///
    /// ```text
    /// | magic | rec_len | version | kind | key_len | val_len | key | value | crc32 |
    /// ```
    pub fn encode(&self) -> Result<Vec<u8>> {
        // 计算总长度
        let rec_len = HEADER_SIZE + self.key.len() + self.value.len() + 4; // +4 for crc32

        // 预分配缓冲区
        let mut buf = Vec::with_capacity(rec_len);

        // 1. 写入 magic
        buf.write_all(&MAGIC)?;

        // 2. 写入 rec_len
        buf.write_all(&(rec_len as u32).to_le_bytes())?;

        // 3. 写入 version
        buf.write_all(&[VERSION])?;

        // 4. 写入 kind
        let kind_byte = match self.kind {
            RecordKind::Put => KIND_PUT,
            RecordKind::Delete => KIND_DELETE,
        };
        buf.write_all(&[kind_byte])?;

        // 5. 写入 key_len
        buf.write_all(&(self.key.len() as u32).to_le_bytes())?;

        // 6. 写入 val_len
        buf.write_all(&(self.value.len() as u32).to_le_bytes())?;

        // 7. 写入 key
        buf.write_all(&self.key)?;

        // 8. 写入 value
        buf.write_all(&self.value)?;

        // 9. 计算 CRC32（覆盖 rec_len..value）
        // 跳过 magic (4 bytes)，从 rec_len 开始计算
        let crc = {
            let mut hasher = Hasher::new();
            hasher.update(&buf[4..]); // 从 rec_len 开始
            hasher.finalize()
        };

        // 10. 写入 crc32
        buf.write_all(&crc.to_le_bytes())?;

        Ok(buf)
    }

    /// 从字节流解码记录
    ///
    /// ## 参数
    ///
    /// - `reader`: 实现了 `Read` trait 的对象（通常是文件）
    ///
    /// ## 返回值
    ///
    /// - `Ok(Some(Record))`: 成功解码一条记录
    /// - `Ok(None)`: 到达文件末尾（正常结束）
    /// - `Err(Error::UnexpectedEof)`: 记录不完整（半写入）
    /// - `Err(Error::CrcMismatch)`: 校验失败（数据损坏）
    /// - `Err(Error::InvalidMagic)`: magic 不匹配（可能不是 WAL 文件）
    ///
    /// ## 解码流程
    ///
    /// 1. 读取 magic (4 bytes)
    /// 2. 读取 rec_len (4 bytes)
    /// 3. 验证 rec_len 是否合理（< MAX_RECORD_SIZE）
    /// 4. 读取剩余字节（rec_len - 8）
    /// 5. 验证 CRC32
    /// 6. 解析字段
    pub fn decode<R: Read>(reader: &mut R) -> Result<Option<Record>> {
        // 1. 读取 magic
        let mut magic = [0u8; 4];
        match reader.read_exact(&mut magic) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // 正常的 EOF（文件结束）
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        }

        // 验证 magic
        if magic != MAGIC {
            return Err(Error::InvalidMagic {
                expected: MAGIC,
                actual: magic,
            });
        }

        // 2. 读取 rec_len
        let mut rec_len_bytes = [0u8; 4];
        reader.read_exact(&mut rec_len_bytes)?;
        let rec_len = u32::from_le_bytes(rec_len_bytes) as usize;

        // 验证 rec_len 是否合理
        if rec_len < HEADER_SIZE + 4 || rec_len > MAX_RECORD_SIZE {
            return Err(Error::UnexpectedEof);
        }

        // 3. 读取剩余数据（rec_len - magic(4) - rec_len(4)）
        let remaining_len = rec_len - 8;
        let mut remaining = vec![0u8; remaining_len];
        reader.read_exact(&mut remaining)?;

        // 4. 验证 CRC32
        // CRC 覆盖 rec_len..value（不包括 magic 和 crc 本身）
        let crc_offset = remaining_len - 4;
        let stored_crc = u32::from_le_bytes([
            remaining[crc_offset],
            remaining[crc_offset + 1],
            remaining[crc_offset + 2],
            remaining[crc_offset + 3],
        ]);

        let computed_crc = {
            let mut hasher = Hasher::new();
            hasher.update(&rec_len_bytes); // rec_len
            hasher.update(&remaining[..crc_offset]); // version..value
            hasher.finalize()
        };

        if stored_crc != computed_crc {
            return Err(Error::CrcMismatch {
                expected: stored_crc,
                actual: computed_crc,
            });
        }

        // 5. 解析字段
        let version = remaining[0];
        if version != VERSION {
            return Err(Error::UnsupportedVersion(version));
        }

        let kind_byte = remaining[1];
        let kind = match kind_byte {
            KIND_PUT => RecordKind::Put,
            KIND_DELETE => RecordKind::Delete,
            _ => return Err(Error::InvalidRecordKind(kind_byte)),
        };

        let key_len = u32::from_le_bytes([
            remaining[2],
            remaining[3],
            remaining[4],
            remaining[5],
        ]) as usize;

        let val_len = u32::from_le_bytes([
            remaining[6],
            remaining[7],
            remaining[8],
            remaining[9],
        ]) as usize;

        // 验证长度
        if key_len > MAX_KEY_SIZE {
            return Err(Error::KeyTooLarge {
                size: key_len,
                max: MAX_KEY_SIZE,
            });
        }
        if val_len > MAX_VALUE_SIZE {
            return Err(Error::ValueTooLarge {
                size: val_len,
                max: MAX_VALUE_SIZE,
            });
        }

        let data_start = 10; // version(1) + kind(1) + key_len(4) + val_len(4)
        let key_start = data_start;
        let key_end = key_start + key_len;
        let val_end = key_end + val_len;

        // 验证数据完整性
        if val_end > crc_offset {
            return Err(Error::UnexpectedEof);
        }

        let key = remaining[key_start..key_end].to_vec();
        let value = remaining[key_end..val_end].to_vec();

        Ok(Some(Record { kind, key, value }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_encode_decode_put() {
        let record = Record::put(b"hello".to_vec(), b"world".to_vec()).unwrap();
        let encoded = record.encode().unwrap();

        let mut cursor = Cursor::new(encoded);
        let decoded = Record::decode(&mut cursor).unwrap().unwrap();

        assert_eq!(record, decoded);
    }

    #[test]
    fn test_encode_decode_delete() {
        let record = Record::delete(b"hello".to_vec()).unwrap();
        let encoded = record.encode().unwrap();

        let mut cursor = Cursor::new(encoded);
        let decoded = Record::decode(&mut cursor).unwrap().unwrap();

        assert_eq!(record, decoded);
    }

    #[test]
    fn test_decode_eof() {
        let mut cursor = Cursor::new(vec![]);
        let result = Record::decode(&mut cursor).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_decode_corrupted_crc() {
        let record = Record::put(b"key".to_vec(), b"value".to_vec()).unwrap();
        let mut encoded = record.encode().unwrap();

        // 损坏最后一个字节（CRC）
        let len = encoded.len();
        encoded[len - 1] ^= 0xFF;

        let mut cursor = Cursor::new(encoded);
        let result = Record::decode(&mut cursor);

        assert!(matches!(result, Err(Error::CrcMismatch { .. })));
    }

    #[test]
    fn test_key_too_large() {
        let large_key = vec![0u8; MAX_KEY_SIZE + 1];
        let result = Record::put(large_key, b"value".to_vec());
        assert!(matches!(result, Err(Error::KeyTooLarge { .. })));
    }

    #[test]
    fn test_value_too_large() {
        let large_value = vec![0u8; MAX_VALUE_SIZE + 1];
        let result = Record::put(b"key".to_vec(), large_value);
        assert!(matches!(result, Err(Error::ValueTooLarge { .. })));
    }
}
