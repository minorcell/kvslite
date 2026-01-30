# kvslite

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/yourusername/kvslite)

**kvslite** 是一个用 Rust 编写的轻量级嵌入式键值存储引擎，基于 Bitcask 模型实现。

## ✨ 特性

- **崩溃安全** - 采用 WAL（Write-Ahead Log）保证数据持久化
- **零依赖** - 除了 `crc32fast`，无其他运行时依赖
- **简单可靠** - 代码清晰，易于理解和审计（~1500 行）
- **嵌入式设计** - 作为库使用，无需独立部署
- **规范注释** - 详细的中文注释和架构说明

## 🚀 快速开始

### 添加依赖

```toml
[dependencies]
kvslite = { path = "../kvslite" }
```

### 基本使用

```rust
use kvslite::{Db, Options};

fn main() -> kvslite::Result<()> {
    // 打开数据库（目录不存在会自动创建）
    let mut db = Db::open("data/db1", Options::default())?;

    // 写入键值
    db.put(b"user:1:name", b"Alice")?;
    db.put(b"user:1:age", b"30")?;

    // 读取值
    let name = db.get(b"user:1:name")?.unwrap();
    println!("Name: {}", String::from_utf8_lossy(&name));

    // 删除键
    db.delete(b"user:1:age")?;

    Ok(())
}
```

### 运行示例

```bash
cargo run --example basic
```

## 📖 架构说明

kvslite v0.1 采用 **Bitcask 架构**：

```
┌──────────────────────────────────────┐
│              Db (API)                │
├──────────────────────────────────────┤
│  - put(key, value)                   │
│  - get(key) -> Option<value>         │
│  - delete(key)                       │
└────────────┬────────────┬────────────┘
             │            │
     ┌───────▼──────┐  ┌──▼──────────┐
     │     WAL      │  │    Index    │
     │  (wal.log)   │  │ (HashMap)   │
     └──────────────┘  └─────────────┘
```

### 核心组件

- **WAL 文件** - 所有写操作追加到 WAL（wal.log）
- **内存索引** - HashMap 记录每个 key 的位置（offset + length）
- **崩溃恢复** - 启动时重放 WAL 重建索引

### 写入流程

1. 编码记录（包含 key、value、CRC32 校验）
2. 追加写入 WAL 文件
3. 执行 flush + fsync（可选）
4. 更新内存索引

### 读取流程

1. 在内存索引中查找 key
2. 根据 offset/length 从 WAL 文件读取 value

### WAL 记录格式

```
+-------+--------+---------+------+----------+----------+-----+-------+--------+
| magic | rec_len| version | kind | key_len  | val_len  | key | value | crc32  |
+-------+--------+---------+------+----------+----------+-----+-------+--------+
  4B      4B       1B       1B      4B         4B        var   var     4B
```

详细设计请参考 [ARCHITECTURE.md](docs/ARCHITECTURE.md)

## ⚙️ 配置选项

```rust
let opts = Options {
    sync_on_write: true,  // 每次写入都 fsync（默认：true）
};
let db = Db::open("data/db1", opts)?;
```

| 选项 | 说明 | 默认值 |
|------|------|--------|
| `sync_on_write` | 每次写入后调用 fsync | `true` |

## 📊 性能特征

v0.1 优化**写入性能**，适合写多读少的场景：

- **写入**：顺序追加，~1000 ops/s（sync=true）
- **读取**：随机 I/O，性能取决于磁盘类型
  - HDD: ~100 ops/s
  - SSD: ~10,000 ops/s

## ⚠️ 限制

v0.1 有以下限制：

| 限制 | 说明 |
|------|------|
| **内存要求** | 所有 key 必须能放入内存 |
| **无范围查询** | 不支持迭代器或前缀扫描 |
| **无事务** | 不支持跨操作的原子性 |
| **单线程** | 写操作需要 `&mut self` |

## 🎯 适用场景

✅ **适合**
- CLI 工具的本地存储
- 嵌入式设备的配置管理
- 小到中等数据集（< 100GB）
- 写多读少的场景

❌ **不适合**
- 大数据集（> 1TB）
- 读密集型应用
- 需要复杂查询（SQL、范围扫描）
- 分布式场景

## 🧪 测试

```bash
# 运行所有测试
cargo test

# 运行单元测试
cargo test --lib

# 运行集成测试
cargo test --test integration_tests
```

测试覆盖：
- 基本操作（put/get/delete）
- 崩溃恢复（WAL replay）
- 边界条件（空 key/value、大 value）
- 数据损坏检测（CRC 校验）

## 📚 文档

- [DESIGN.md](docs/DESIGN.md) - 用户使用指南
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) - 内部设计文档（面向开发者）
- [REVIEW.md](docs/REVIEW.md) - 产品与架构评审

## 🗓️ 路线图

| 版本 | 主要功能 | 状态 |
|------|----------|------|
| **v0.1** | WAL + 内存索引 + 崩溃恢复（MVP） | ✅ 完成 |
| **v0.2** | WAL 压缩 + 统计信息 + 文件锁 | 📋 计划中 |
| **v0.3** | MemTable + SSTable（LSM 架构） | 📋 计划中 |

## 📝 许可证

MIT OR Apache-2.0

---

## 💡 代码解读

### 1. 为什么需要两个文件句柄？

```rust
pub struct Wal {
    write_file: File,  // 追加写入
    read_file: File,   // 随机读取
}
```

**原因**：
- `write_file` 以 append 模式打开，只能追加
- `read_file` 以 read 模式打开，支持 seek
- 分离读写避免相互干扰

### 2. 为什么 CRC 要覆盖 rec_len？

```rust
// CRC 覆盖 rec_len..value
hasher.update(&rec_len_bytes);
hasher.update(&remaining[..crc_offset]);
```

**原因**：
- 如果 `rec_len` 损坏（bit flip），CRC 仍会通过
- 可能读取错误长度的数据，导致内存耗尽
- 将 `rec_len` 纳入校验范围可以检测所有字段损坏

### 3. 为什么索引重建要重新编码？

```rust
fn rebuild_index(records: &[Record]) -> HashMap<Vec<u8>, ValuePos> {
    let encoded = record.encode().unwrap();  // 重新编码
    let record_len = encoded.len() as u64;
    // ...
}
```

**原因**：
- Replay 返回的是 `Record` 对象，不包含位置信息
- 需要知道每条记录的大小才能计算累积偏移量
- v0.1 优先正确性，未来可优化（在 replay 时直接返回位置）

### 4. 为什么删除不存在的 key 也要写 WAL？

```rust
pub fn delete(&mut self, key: &[u8]) -> Result<()> {
    let record = Record::delete(key.to_vec())?;
    self.wal.append(&record, self.opts.sync_on_write)?;  // 总是写入
    self.index.remove(key);
    Ok(())
}
```

**原因**：
- 保证操作的持久化语义
- 即使 key 不存在，删除操作也应该被记录
- 避免"幻读"（重启后突然出现已删除的 key）

## 🤝 贡献

欢迎贡献！请先阅读 [ARCHITECTURE.md](docs/ARCHITECTURE.md) 了解内部设计。

## 🙏 致谢

- Bitcask 论文：[设计灵感来源](https://riak.com/assets/bitcask-intro.pdf)
- LevelDB：LSM 架构参考
