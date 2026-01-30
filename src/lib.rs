//! # kvslite
//!
//! kvslite 是一个轻量级的嵌入式键值存储引擎，基于 Bitcask 模型实现。
//!
//! ## 设计特点
//!
//! - **崩溃安全**：采用 WAL（Write-Ahead Log）保证数据持久化
//! - **简单可靠**：代码简洁，易于理解和审计
//! - **嵌入式**：作为库使用，无需独立部署
//!
//! ## 快速开始
//!
//! ```no_run
//! use kvslite::{Db, Options};
//!
//! let mut db = Db::open("data/db1", Options::default()).unwrap();
//!
//! // 写入键值
//! db.put(b"key", b"value").unwrap();
//!
//! // 读取值
//! let value = db.get(b"key").unwrap();
//! assert_eq!(value.as_deref(), Some(b"value" as &[u8]));
//!
//! // 删除键
//! db.delete(b"key").unwrap();
//! ```
//!
//! ## 架构说明
//!
//! kvslite v0.1 采用 Bitcask 架构：
//!
//! - **WAL 文件**：所有写操作追加到 WAL（Write-Ahead Log）
//! - **内存索引**：HashMap 记录每个 key 的位置（offset + length）
//! - **崩溃恢复**：启动时重放 WAL 重建索引
//!
//! ### 写入流程
//!
//! 1. 编码记录（Record）
//! 2. 追加写入 WAL 文件
//! 3. 执行 flush + fsync（可选）
//! 4. 更新内存索引
//!
//! ### 读取流程
//!
//! 1. 在内存索引中查找 key
//! 2. 根据 offset/length 从 WAL 文件读取 value
//!
//! ## 限制
//!
//! v0.1 有以下限制：
//!
//! - 所有 key 必须能放入内存
//! - 不支持范围查询
//! - 不支持事务
//! - 单线程写入（`&mut self` 语义）

mod codec;
mod db;
mod error;
mod wal;

// 对外导出核心类型
pub use db::{Db, Options};
pub use error::{Error, Result};
