# kvlite

**kvlite** 是一个用 Rust 编写的轻量级嵌入式键值存储引擎，定位类似 SQLite：无需部署服务，直接作为 crate 嵌入到 Rust 项目中使用。

> **设计目标：** 崩溃安全 + 易于嵌入 + 可持续演进

---

## 特性

- **嵌入式设计** - 作为 Rust crate 使用，无需网络部署
- **简洁 API** - 提供 `open / put / get / delete` 基础操作
- **持久化存储** - 数据安全落盘到本地目录
- **崩溃恢复** - 支持断电/kill -9 后恢复到最后一次完整写入
- **简单可维护** - 代码优先正确性，再考虑优化

---

## 当前限制

以下功能暂不支持（未来版本可能加入）：

- 网络服务与分布式特性
- 多机复制、Raft 共识
- SQL 查询与二级索引
- 多进程并发访问（v0.x 阶段）
- 事务与 MVCC

---

## 快速开始

### 添加依赖

在 `Cargo.toml` 中添加：

```toml
[dependencies]
kvlite = { path = "../kvlite" }
```

### 使用示例

```rust
use kvlite::{Db, Options};

fn main() -> kvlite::Result<()> {
    // 打开数据库（目录不存在会自动创建）
    let mut db = Db::open("data/db1", Options::default())?;

    // 写入键值
    db.put(b"alice", b"100")?;

    // 读取值
    assert_eq!(db.get(b"alice")?.as_deref(), Some(b"100"));

    // 删除键
    db.delete(b"alice")?;
    assert_eq!(db.get(b"alice")?, None);

    Ok(())
}
```

---

## API 说明

### 打开数据库

```rust
Db::open(path: impl AsRef<Path>, opts: Options) -> Result<Db>
```

**参数：**

- `path` - 数据库目录路径（不存在会自动创建）
- `opts` - 引擎配置选项

### 基本操作

```rust
// 写入键值对
db.put(key: &[u8], value: &[u8]) -> Result<()>

// 读取值（返回 None 表示键不存在）
db.get(key: &[u8]) -> Result<Option<Vec<u8>>>

// 删除键
db.delete(key: &[u8]) -> Result<()>
```

---

## 持久化与崩溃安全

kvlite 采用 **WAL（Write-Ahead Log）追加写**机制保证数据持久化。

### 默认行为

- 配置 `sync_on_write = true`（默认）
- `put/delete` 返回 `Ok(())` 表示数据已安全落盘
- 即使发生崩溃或断电，重启后也能恢复到最后一次完整写入

### 数据目录结构

**当前版本 (v0.1)：**

```
db_dir/
  wal.log
```

**后续版本将包含：**

```
db_dir/
  LOCK
  MANIFEST
  wal.log
  sst/
```

---

## 开发路线图

| 版本     | 主要功能                         |
| -------- | -------------------------------- |
| **v0.1** | WAL + 内存索引 + 崩溃恢复（MVP） |
| **v0.2** | WAL 压缩 + 统计信息 + 文件锁     |
| **v0.3** | MemTable + SSTable（LSM 架构）   |
| **v0.4** | 后台 Compaction（合并 SSTable）  |
| **v0.5** | 缓存 + Bloom Filter + 性能优化   |
| **v0.6** | 并发支持（多读 + 单写）          |

---

## 相关文档

- [ARCHITECTURE.md](./ARCHITECTURE.md) - 内部设计与实现细节（面向开发者）

---

## 许可证

MIT OR Apache-2.0
