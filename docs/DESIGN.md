# kvslite 设计文档

## 项目概述

kvslite 是一个极简的文件持久化键值存储库，设计灵感来自浏览器的 `localStorage` API。它提供了简单直接的同步接口，将数据以人类可读的 JSON 格式存储在本地文件中。

## 设计目标

### 核心原则

1. **简单优先**：API 面积极小，只提供必要的操作
2. **同步设计**：每次修改立即持久化，无需手动 flush
3. **可读性**：使用 JSON 格式，便于调试和手工编辑
4. **数据安全**：通过原子写入避免文件损坏
5. **零配置**：无需预先创建目录或初始化数据库

### 非目标

- **不追求高性能**：不适合高频写入场景
- **不支持并发**：无进程间锁机制
- **不支持复杂查询**：仅支持简单的 key-value 操作
- **不支持大数据**：全量加载到内存，不适合大规模数据

## 架构设计

### 数据模型

```rust
pub struct KvStore {
    path: PathBuf,              // 存储文件路径
    data: HashMap<String, String>, // 内存中的键值对
}
```

**设计说明**：

- 使用 `HashMap<String, String>` 作为内存存储，限定 key 和 value 都是字符串
- 通过 `PathBuf` 持有文件路径，用于后续的持久化操作
- 采用"读取-修改-写入"模式，所有数据保存在内存中

### API 设计

#### 核心接口

```rust
// 创建/打开存储
pub fn open(path: impl AsRef<Path>) -> Result<Self>

// 写入键值
pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> Result<()>

// 删除键
pub fn remove(&mut self, key: &str) -> Result<()>

// 清空所有数据
pub fn clear(&mut self) -> Result<()>

// 读取值（不触发磁盘 I/O）
pub fn get(&self, key: &str) -> Option<&str>
```

**设计考虑**：

1. **`open` 接受泛型参数**：支持 `&str`、`String`、`&Path`、`PathBuf` 等多种路径类型
2. **`set` 使用 `Into<String>`**：允许传入 `&str` 或 `String`，提高易用性
3. **`get` 返回 `Option<&str>`**：避免不必要的内存分配，返回引用
4. **`set/remove/clear` 需要 `&mut self`**：表明会修改内部状态
5. **同步持久化**：所有修改操作立即调用 `persist()`

### 持久化机制

#### 加载流程 (`load`)

```
1. 确保父目录存在
   └─> ensure_parent_dir(path)

2. 检查文件是否存在
   ├─> 不存在：返回空 HashMap
   └─> 存在：继续

3. 读取文件内容
   └─> fs::read_to_string(path)

4. 检查内容是否为空
   ├─> 空白：返回空 HashMap
   └─> 非空：JSON 反序列化
```

**设计亮点**：

- 提前创建父目录，避免后续写入失败
- 优雅处理空文件和不存在文件的情况
- 使用 `?` 操作符统一错误处理

#### 持久化流程 (`persist`)

```
1. 确保父目录存在
   └─> ensure_parent_dir(path)

2. 序列化数据为 JSON
   └─> serde_json::to_string_pretty(&self.data)

3. 生成临时文件路径
   └─> path.with_extension("tmp")

4. 写入临时文件
   └─> fs::write(&tmp_path, json)

5. 原子替换
   └─> fs::rename(tmp_path, &self.path)
```

**安全保证**：

- **原子写入**：先写临时文件，再通过 `rename` 原子替换
- **避免损坏**：即使写入过程中断，原文件保持完整
- **可读格式**：使用 `to_string_pretty` 生成格式化 JSON

#### 辅助函数

```rust
fn ensure_parent_dir(path: &Path) -> Result<()>
```

**职责**：确保文件的父目录存在，如果不存在则递归创建。

**实现细节**：

- 检查 `path.parent()` 是否为空路径（如当前目录的 `"file.json"`）
- 使用 `create_dir_all` 递归创建多级目录
- 在 `load` 和 `persist` 中复用，消除重复代码

### 错误处理

```rust
pub enum KvError {
    Io(io::Error),              // I/O 错误
    Serde(serde_json::Error),   // JSON 序列化/反序列化错误
}
```

**设计思路**：

- 简单的枚举类型，覆盖两种主要错误来源
- 实现 `From<io::Error>` 和 `From<serde_json::Error>`，支持 `?` 操作符
- 实现 `Display` 和 `std::error::Error`，符合 Rust 错误处理惯例

## 使用场景分析

### 适合场景

1. **CLI 工具配置**

   ```rust
   let mut config = KvStore::open("~/.myapp/config.json")?;
   config.set("api_key", user_input)?;
   config.set("theme", "dark")?;
   ```

2. **测试数据存储**

   ```rust
   let mut cache = KvStore::open("test_data/cache.json")?;
   cache.set("last_run", timestamp)?;
   ```

3. **简单状态持久化**
   ```rust
   let mut state = KvStore::open("state.json")?;
   state.set("current_user", username)?;
   ```

### 不适合场景

1. **高频写入**：每次修改都会触发文件 I/O
2. **大数据集**：全量加载到内存
3. **多进程访问**：无并发控制机制
4. **需要事务**：不支持批量操作和回滚
5. **需要查询**：只支持精确键查找

## 性能考虑

### 时间复杂度

- `get(key)`: O(1) - HashMap 查找
- `set(key, value)`: O(n) - HashMap 插入 O(1) + 序列化整个数据集 O(n) + 写文件
- `remove(key)`: O(n) - HashMap 删除 O(1) + 序列化整个数据集 O(n) + 写文件
- `clear()`: O(1) - 清空 HashMap + 写入空对象

### 空间复杂度

- 内存占用：O(n)，n 为所有 key-value 对的总大小
- 磁盘占用：约 1.1x 内存数据（JSON 格式开销）

### 优化建议

如果需要更高性能，可以考虑：

1. **批量操作**：添加 `batch_set` 方法，延迟持久化
2. **异步 I/O**：使用 `tokio::fs` 实现异步版本
3. **增量持久化**：记录变更日志，而非全量写入
4. **压缩**：对 JSON 数据进行压缩存储

## 并发与线程安全

### 当前状态

- `KvStore` **不是** `Send` 或 `Sync`（因为包含 `PathBuf` 和 `HashMap`）
- 实际上 `KvStore` 可以是 `Send`，但**不应该跨线程共享**

### 并发场景

1. **单进程单线程**：✅ 完全安全
2. **单进程多线程**：需要用 `Mutex` 或 `RwLock` 包装
3. **多进程**：❌ 不安全，可能导致数据丢失或损坏

**建议**：

- 每个进程/线程维护自己的 `KvStore` 实例
- 如需多进程共享，使用文件锁（如 `fs2` crate）

## 测试策略

### 现有测试

1. **基本读写测试** (`set_and_read_back`)
   - 验证数据持久化
   - 验证跨实例读取

2. **删除和清空测试** (`remove_and_clear`)
   - 验证 `remove` 操作
   - 验证 `clear` 操作

### 建议补充测试

1. **边界情况**
   - 空 key 或 value
   - 超长字符串
   - 特殊字符（Unicode、控制字符）

2. **错误处理**
   - 无权限目录
   - 磁盘空间不足
   - 损坏的 JSON 文件

3. **文件系统交互**
   - 嵌套目录自动创建
   - 临时文件清理
   - 原子替换验证

## 未来扩展方向

### 短期优化

1. **批量操作 API**

   ```rust
   pub fn batch_update<F>(&mut self, f: F) -> Result<()>
   where F: FnOnce(&mut HashMap<String, String>)
   ```

2. **迭代器支持**

   ```rust
   pub fn iter(&self) -> impl Iterator<Item = (&str, &str)>
   pub fn keys(&self) -> impl Iterator<Item = &str>
   pub fn values(&self) -> impl Iterator<Item = &str>
   ```

3. **包含检查**
   ```rust
   pub fn contains_key(&self, key: &str) -> bool
   pub fn len(&self) -> usize
   pub fn is_empty(&self) -> bool
   ```

### 中期功能

1. **异步版本**：`AsyncKvStore` 基于 `tokio`
2. **泛型值类型**：支持 `serde::Serialize + serde::Deserialize`
3. **监听变更**：文件监听 + 自动重载
4. **加密存储**：可选的数据加密

### 长期演进

1. **索引优化**：支持前缀查询、范围查询
2. **压缩存储**：可配置的压缩算法
3. **多文件分片**：突破单文件大小限制
4. **Write-Ahead Log**：提升崩溃恢复能力

## 替代方案对比

| 方案          | 优势                   | 劣势               | 适用场景         |
| ------------- | ---------------------- | ------------------ | ---------------- |
| **kvslite**   | 零配置，人类可读，简单 | 性能一般，无并发   | CLI 配置，小工具 |
| **sled**      | 高性能，支持并发       | 二进制格式，较复杂 | 本地数据库需求   |
| **sqlite**    | 标准 SQL，成熟稳定     | 需要 SQL 知识      | 结构化数据存储   |
| **rocksdb**   | 极高性能，可扩展       | 重量级，学习成本高 | 大规模 KV 存储   |
| **JSON 文件** | 最简单                 | 需手动处理 I/O     | 一次性配置       |

## 总结

kvslite 是一个**专注于简单性**的本地持久化存储方案。通过牺牲性能和并发能力，换取极简的 API 和零学习成本的开发体验。它适合配置存储、状态缓存等低频访问场景，不适合作为高性能数据库使用。

核心设计哲学：**够用就好，简单至上**。
