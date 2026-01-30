use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// kvslite 操作的便捷返回类型。
pub type Result<T> = std::result::Result<T, KvError>;

/// 覆盖 I/O 与 JSON 序列化错误的轻量错误类型。
#[derive(Debug)]
pub enum KvError {
    Io(io::Error),
    Serde(serde_json::Error),
}

impl Display for KvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            KvError::Io(err) => write!(f, "I/O error: {err}"),
            KvError::Serde(err) => write!(f, "Serde error: {err}"),
        }
    }
}

impl std::error::Error for KvError {}

impl From<io::Error> for KvError {
    fn from(value: io::Error) -> Self {
        KvError::Io(value)
    }
}

impl From<serde_json::Error> for KvError {
    fn from(value: serde_json::Error) -> Self {
        KvError::Serde(value)
    }
}

/// 简洁的文件持久化 KV 存储，灵感来自浏览器 `localStorage`。
///
/// 仅提供 `set`、`remove`、`clear` 三个同步接口；每次修改都会落盘为 JSON，
/// 因此使用同一路径新建实例能读取到最新数据。
pub struct KvStore {
    path: PathBuf,
    data: HashMap<String, String>,
}

impl KvStore {
    /// 打开（或创建）给定文件路径的存储。
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let data = Self::load(&path)?;
        Ok(Self { path, data })
    }

    /// 写入或覆盖键值，并立即持久化。
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> Result<()> {
        self.data.insert(key.into(), value.into());
        self.persist()
    }

    /// 删除键（不存在则忽略），并立即持久化。
    pub fn remove(&mut self, key: &str) -> Result<()> {
        self.data.remove(key);
        self.persist()
    }

    /// 清空所有数据，并立即持久化。
    pub fn clear(&mut self) -> Result<()> {
        self.data.clear();
        self.persist()
    }

    /// 只读查看内存中的值，不触碰磁盘，便于测试或调用方校验。
    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(|s| s.as_str())
    }

    /// 确保文件的父目录存在。
    fn ensure_parent_dir(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        Ok(())
    }

    fn load(path: &Path) -> Result<HashMap<String, String>> {
        Self::ensure_parent_dir(path)?;

        if !path.exists() {
            return Ok(HashMap::new());
        }

        let contents = fs::read_to_string(path)?;
        if contents.trim().is_empty() {
            return Ok(HashMap::new());
        }

        Ok(serde_json::from_str(&contents)?)
    }

    fn persist(&self) -> Result<()> {
        Self::ensure_parent_dir(&self.path)?;

        let json = serde_json::to_string_pretty(&self.data)?;
        let tmp_path = self.path.with_extension("tmp");

        // 先写临时文件再原子替换，避免部分写入导致的损坏。
        fs::write(&tmp_path, json)?;
        fs::rename(tmp_path, &self.path)?;
        Ok(())
    }
}
