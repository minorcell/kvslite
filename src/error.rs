//! 错误类型定义
//!
//! 本模块定义了 kvslite 的所有错误类型。
//!
//! ## 设计原则
//!
//! - 使用标准库的 `Error` trait
//! - 错误信息清晰，便于调试
//! - 支持从标准 I/O 错误转换

use std::fmt;
use std::io;

/// kvslite 的错误类型
#[derive(Debug)]
pub enum Error {
    /// I/O 错误（文件读写、目录创建等）
    Io(io::Error),

    /// 数据损坏：CRC 校验失败
    ///
    /// 包含期望的 CRC 值和实际计算的 CRC 值
    CrcMismatch {
        expected: u32,
        actual: u32,
    },

    /// 数据损坏：无效的 Magic 字节
    ///
    /// WAL 记录必须以 "KVSL" 开头
    InvalidMagic {
        expected: [u8; 4],
        actual: [u8; 4],
    },

    /// 数据损坏：不支持的版本号
    UnsupportedVersion(u8),

    /// 数据损坏：无效的记录类型
    ///
    /// 当前只支持 PUT (1) 和 DELETE (2)
    InvalidRecordKind(u8),

    /// 数据不完整：WAL 文件意外结束
    ///
    /// 通常发生在崩溃导致的半写入（torn write）
    UnexpectedEof,

    /// 键或值过大
    ///
    /// v0.1 限制：
    /// - key 最大 1KB
    /// - value 最大 1MB
    ValueTooLarge {
        size: usize,
        max: usize,
    },

    /// 键过大
    KeyTooLarge {
        size: usize,
        max: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::CrcMismatch { expected, actual } => {
                write!(f, "CRC mismatch: expected {:#x}, got {:#x}", expected, actual)
            }
            Error::InvalidMagic { expected, actual } => {
                write!(
                    f,
                    "Invalid magic: expected {:?}, got {:?}",
                    expected, actual
                )
            }
            Error::UnsupportedVersion(v) => {
                write!(f, "Unsupported version: {}", v)
            }
            Error::InvalidRecordKind(k) => {
                write!(f, "Invalid record kind: {}", k)
            }
            Error::UnexpectedEof => {
                write!(f, "Unexpected EOF while reading record")
            }
            Error::ValueTooLarge { size, max } => {
                write!(f, "Value too large: {} bytes (max {})", size, max)
            }
            Error::KeyTooLarge { size, max } => {
                write!(f, "Key too large: {} bytes (max {})", size, max)
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// 从标准 I/O 错误自动转换
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// kvslite 的 Result 类型别名
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::CrcMismatch {
            expected: 0x1234,
            actual: 0x5678,
        };
        assert_eq!(
            err.to_string(),
            "CRC mismatch: expected 0x1234, got 0x5678"
        );
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }
}
