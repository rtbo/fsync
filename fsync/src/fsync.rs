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
            path: PathBuf::root(),
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

    impl Entry {
        pub fn root() -> Self {
            Self::Both {
                local: super::Metadata::root(),
                remote: super::Metadata::root(),
            }
        }

        pub fn path(&self) -> &Path {
            match self {
                Entry::Both { local, remote } => {
                    debug_assert_eq!(local.path(), remote.path());
                    local.path()
                }
                Entry::Local(entry) => entry.path(),
                Entry::Remote(entry) => entry.path(),
            }
        }

        pub fn into_local_metadata(self) -> Option<super::Metadata> {
            match self {
                Self::Local(metadata) => Some(metadata),
                Self::Remote(..) => None,
                Self::Both { local, .. } => Some(local),
            }
        }

        pub fn into_remote_metadata(self) -> Option<super::Metadata> {
            match self {
                Self::Local(..) => None,
                Self::Remote(metadata) => Some(metadata),
                Self::Both { remote, .. } => Some(remote),
            }
        }

        pub fn is_local_only(&self) -> bool {
            matches!(self, Entry::Local(..))
        }

        pub fn is_remote_only(&self) -> bool {
            matches!(self, Entry::Remote(..))
        }

        pub fn is_both(&self) -> bool {
            matches!(self, Entry::Both { .. })
        }
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

        pub fn into_entry(self) -> Entry {
            self.entry
        }

        pub fn children(&self) -> &[String] {
            &self.children
        }

        pub fn path(&self) -> &Path {
            self.entry.path()
        }

        pub fn is_local_only(&self) -> bool {
            self.entry.is_local_only()
        }

        pub fn is_remote_only(&self) -> bool {
            self.entry.is_remote_only()
        }

        pub fn is_both(&self) -> bool {
            self.entry.is_both()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    CopyRemoteToLocal(PathBuf),
    CopyLocalToRemote(PathBuf),
    ReplaceLocalByRemote(PathBuf),
    ReplaceRemoteByLocal(PathBuf),
    DeleteLocal(PathBuf),
}

#[tarpc::service]
pub trait Fsync {
    async fn entry(path: PathBuf) -> crate::Result<Option<tree::Node>>;
    async fn operate(operation: Operation) -> crate::Result<()>;
}
