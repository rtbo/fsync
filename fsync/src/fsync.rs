use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Metadata {
    Directory {
        path: PathBuf,
    },
    Regular {
        path: PathBuf,
        size: u64,
        mtime: DateTime<Utc>,
    },
    Symlink {
        path: PathBuf,
        target: String,
        size: u64,
        mtime: Option<DateTime<Utc>>,
    },
    Special {
        path: PathBuf,
    },
}

impl Metadata {
    pub fn root() -> Self {
        Self::Directory {
            path: Default::default(),
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Directory { path, .. } => path,
            Self::Regular { path, .. } => path,
            Self::Symlink { path, .. } => path,
            Self::Special { path, .. } => path,
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
        matches!(self, Self::Directory { .. })
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::Regular { .. })
    }

    pub fn is_symlink(&self) -> bool {
        matches!(self, Self::Symlink { .. })
    }

    pub fn is_special(&self) -> bool {
        matches!(self, Self::Special { .. })
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

#[derive(Debug, Copy, Clone)]
pub struct PathId<'a> {
    pub id: &'a str,
    pub path: &'a Path,
}

impl<'a> PathId<'a> {
    pub fn to_path_id_buf(&self) -> PathIdBuf {
        PathIdBuf {
            id: self.id.into(),
            path: self.path.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PathIdBuf {
    pub id: String,
    pub path: PathBuf,
}

impl PathIdBuf {
    pub fn as_path_id(&self) -> PathId<'_> {
        PathId {
            id: &self.id,
            path: &self.path,
        }
    }
}

impl<'a> From<PathId<'a>> for PathIdBuf {
    fn from(pid: PathId<'a>) -> Self {
        pid.to_path_id_buf()
    }
}

pub mod tree {
    use serde::{Deserialize, Serialize};

    use crate::path::Path;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum Entry {
        Local(super::Metadata),
        Remote(super::Metadata),
        Both {
            local: super::Metadata,
            remote: super::Metadata,
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Node {
        entry: Entry,
        children: Vec<String>,
    }

    impl Node {
        pub fn new(entry: Entry, children: Vec<String>) -> Self {
            Self { entry, children }
        }

        pub fn entry(&self) -> &Entry {
            &self.entry
        }

        pub fn children(&self) -> &[String] {
            &self.children
        }

        pub fn path(&self) -> &Path {
            match &self.entry {
                Entry::Both { local, remote } => {
                    debug_assert_eq!(local.path(), remote.path());
                    local.path()
                }
                Entry::Local(entry) => entry.path(),
                Entry::Remote(entry) => entry.path(),
            }
        }

        pub fn is_local_only(&self) -> bool {
            matches!(self.entry, Entry::Local(..))
        }

        pub fn is_remote_only(&self) -> bool {
            matches!(self.entry, Entry::Remote(..))
        }

        pub fn is_both(&self) -> bool {
            matches!(self.entry, Entry::Both { .. })
        }
    }
}

#[tarpc::service]
pub trait Fsync {
    async fn entry(path: Option<PathBuf>) -> Option<tree::Node>;
    async fn copy_remote_to_local(path: PathBuf) -> Result<(), String>;
}
