use camino::{Utf8PathBuf, Utf8Path};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Metadata {
    Directory(Utf8PathBuf),
    Regular {
        path: Utf8PathBuf,
        size: u64,
        mtime: DateTime<Utc>,
    },
    Symlink {
        path: Utf8PathBuf,
        target: String,
        size: u64,
        mtime: Option<DateTime<Utc>>,
    },
    Special(Utf8PathBuf),
}

impl Metadata {
    pub fn root() -> Self {
        Self::Directory("".into())
    }

    pub fn path(&self) -> &Utf8Path {
        match self {
            Self::Directory(path) => path,
            Self::Regular{path, ..} => path,
            Self::Symlink{path, ..} => path,
            Self::Special(path) => path,
        }
    }

    pub fn path_or_root(&self) -> &str {
        if self.path().as_str().is_empty() {
            "(root)"
        } else {
            self.path().as_str()
        }
    }

    pub fn name(&self) -> &str {
        self.path().file_name().unwrap_or("")
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Self::Directory(..))
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::Regular { .. })
    }

    pub fn is_symlink(&self) -> bool {
        matches!(self, Self::Symlink { .. })
    }

    pub fn is_special(&self) -> bool {
        matches!(self, Self::Special(..))
    }

    pub fn size(&self) -> Option<u64> {
        match self {
            Self::Regular { size, .. } => Some(*size),
            Self::Symlink { size, .. } => Some(*size),
            _ => None,
        }
    }

    pub fn mtime(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Regular { mtime, .. } => Some(*mtime),
            Self::Symlink { mtime, .. } => *mtime,
            _ => None,
        }
    }

    pub fn symlink_target(&self) -> Option<&str> {
        match &self {
            Self::Symlink { target, .. } => Some(target),
            _ => None,
        }
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Metadata::root()
    }
}

#[tarpc::service]
pub trait Fsync {
    async fn entry(path: Option<Utf8PathBuf>) -> Option<tree::Node>;
    async fn copy_remote_to_local(path: Utf8PathBuf) -> Result<(), String>;
}