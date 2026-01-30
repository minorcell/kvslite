# kvslite

本地文件持久化的极简键值存储，像 Rust 版 `localStorage`。

适合 CLI 工具、小型服务或脚本保存配置、token、少量状态。零依赖数据库，开箱即用。

## 特性

- **极简 API**：只有 `open`、`set`、`get`、`remove`、`clear` 五个方法
- **同步设计**：每次修改立即持久化，无需手动 flush
- **人类可读**：数据存储为格式化 JSON，便于调试和手工编辑
- **安全写入**：使用临时文件 + 原子替换，避免写入中断导致数据损坏
- **零配置**：自动创建目录和文件，无需预先初始化

## 安装

```bash
cargo add kvslite
```

或手动添加到 `Cargo.toml`：

```toml
[dependencies]
kvslite = "0.1.0"
```

## 快速开始

```rust
use kvslite::KvStore;

fn main() -> Result<(), kvslite::KvError> {
    // 打开或创建存储（自动创建父目录）
    let mut store = KvStore::open("data/config.json")?;

    // 写入数据（立即持久化）
    store.set("api_token", "sk-1234567890")?;
    store.set("theme", "dark")?;
    store.set("user_name", "alice")?;

    // 读取数据
    if let Some(token) = store.get("api_token") {
        println!("Token: {}", token);
    }

    // 删除数据
    store.remove("user_name")?;

    // 清空所有数据
    store.clear()?;

    Ok(())
}
```

## API 文档

### `KvStore::open(path)`

打开或创建一个键值存储。

```rust
let mut store = KvStore::open("config.json")?;
let mut store = KvStore::open("/etc/myapp/data.json")?;
let mut store = KvStore::open("./data/store.json")?;
```

**行为**：

- 如果文件不存在，创建空存储
- 如果文件存在，加载其中的数据
- 自动创建所需的父目录

### `store.set(key, value)`

写入或更新键值对，并立即持久化到磁盘。

```rust
store.set("name", "Bob")?;
store.set("age", "30")?;
store.set("email", "bob@example.com")?;
```

**参数**：

- `key`: 字符串或可转换为字符串的类型
- `value`: 字符串或可转换为字符串的类型

### `store.get(key)`

读取键对应的值，返回 `Option<&str>`。

```rust
match store.get("name") {
    Some(value) => println!("Name: {}", value),
    None => println!("Key not found"),
}
```

**特点**：

- 纯内存操作，不触发磁盘 I/O
- 返回引用，避免内存分配

### `store.remove(key)`

删除指定的键，并立即持久化。

```rust
store.remove("deprecated_key")?;
```

**行为**：

- 如果键不存在，不会报错

### `store.clear()`

清空所有数据，并立即持久化。

```rust
store.clear()?;
```

## 使用场景

### 适合场景

- **CLI 工具配置**：保存 API token、用户偏好设置

  ```rust
  let mut config = KvStore::open("~/.myapp/config.json")?;
  config.set("api_key", user_input)?;
  ```

- **开发环境状态**：保存临时数据、调试信息

  ```rust
  let mut cache = KvStore::open("dev_cache.json")?;
  cache.set("last_build", timestamp)?;
  ```

- **测试数据存储**：单元测试或集成测试的数据持久化

  ```rust
  let mut test_data = KvStore::open("test/fixtures.json")?;
  test_data.set("mock_user_id", "12345")?;
  ```

- **脚本自动化**：简单任务的状态记录
  ```rust
  let mut state = KvStore::open("script_state.json")?;
  state.set("last_processed", file_path)?;
  ```

### 不适合场景

- **高频写入**：每次修改都触发完整的 JSON 序列化和文件 I/O
- **大数据集**：所有数据加载到内存，不适合 GB 级数据
- **多进程并发**：无进程间锁机制，可能导致数据覆盖
- **复杂查询**：只支持精确键查找，不支持范围查询或模糊搜索
- **事务需求**：不支持批量操作和回滚

## 数据格式

数据以格式化 JSON 存储，可以直接用文本编辑器查看和修改：

```json
{
  "api_token": "sk-1234567890",
  "theme": "dark",
  "last_login": "2026-01-30T10:30:00Z"
}
```

## 错误处理

所有修改操作返回 `Result<(), KvError>`：

```rust
pub enum KvError {
    Io(io::Error),              // 文件 I/O 错误
    Serde(serde_json::Error),   // JSON 序列化/反序列化错误
}
```

示例：

```rust
use kvslite::{KvStore, KvError};

fn run() -> Result<(), KvError> {
    let mut store = KvStore::open("data.json")?;
    store.set("key", "value")?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
    }
}
```

## 持久化机制

每次调用 `set`、`remove` 或 `clear` 时，kvslite 使用以下流程确保数据安全：

1. 将数据序列化为 JSON
2. 写入临时文件（`path.tmp`）
3. 原子性地将临时文件重命名为目标文件

这种机制保证即使写入过程中程序崩溃，原始数据文件也不会损坏。

## 性能考虑

- **读取性能**：`get()` 是 O(1) 的 HashMap 查找，极快
- **写入性能**：`set()` / `remove()` 需要序列化整个数据集并写入磁盘，O(n)
- **内存占用**：所有数据加载到内存，约等于 JSON 文件大小

**建议**：

- 数据量保持在 10MB 以内
- 写入频率低于每秒 10 次
- 单次操作的数据变更不超过 1000 个键值对

## 并发与线程安全

- **单线程**：完全安全
- **多线程**：需要用 `Mutex` 或 `RwLock` 包装
  ```rust
  use std::sync::Mutex;
  let store = Mutex::new(KvStore::open("data.json")?);
  ```
- **多进程**：不安全，可能导致数据丢失

如需多进程访问，建议使用文件锁（如 `fs2` crate）或考虑其他方案（sled、SQLite 等）。

## 设计文档

详细的架构设计、实现原理和技术选型请参阅 [DESIGN.md](DESIGN.md)，包括：

- 数据模型与 API 设计
- 持久化流程详解
- 性能分析与优化建议
- 并发场景说明
- 未来扩展方向
- 与其他方案的对比

## 测试

运行测试：

```bash
cargo test
```

测试覆盖：

- 基本读写和持久化
- 删除和清空操作
- 跨实例数据加载

## 示例项目

查看 `examples/` 目录（TODO）获取更多实际应用示例：

- `cli_config.rs` - CLI 工具配置管理
- `cache.rs` - 简单缓存实现
- `session.rs` - 用户会话存储

## 替代方案

如果 kvslite 不满足您的需求，可以考虑：

| 方案                                                    | 适用场景         | 优势                |
| ------------------------------------------------------- | ---------------- | ------------------- |
| [sled](https://github.com/spacejam/sled)                | 高性能本地数据库 | 支持并发，ACID 事务 |
| [redb](https://github.com/cberner/redb)                 | 嵌入式键值数据库 | 纯 Rust，零依赖     |
| [SQLite](https://github.com/rusqlite/rusqlite)          | 结构化数据       | 标准 SQL，成熟稳定  |
| [RocksDB](https://github.com/rust-rocksdb/rust-rocksdb) | 大规模 KV 存储   | 极高性能，可扩展    |

## 许可证

MIT 或 Apache-2.0，任选其一。

## 贡献

欢迎提交 Issue 和 Pull Request！

## 更新日志

### 0.1.0 (2026-01-30)

- 初始版本发布
- 基础 CRUD 操作
- 原子写入机制
- JSON 持久化
