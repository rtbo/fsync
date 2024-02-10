use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    conflict::Conflict,
    path::{Path, PathBuf},
    Location,
};

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

    use crate::{conflict::ConflictTy, path::Path};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum Entry {
        Local(super::Metadata),
        Remote(super::Metadata),
        Sync {
            local: super::Metadata,
            remote: super::Metadata,
        },
    }

    impl Entry {
        pub fn root() -> Self {
            Self::Sync {
                local: super::Metadata::root(),
                remote: super::Metadata::root(),
            }
        }

        pub fn path(&self) -> &Path {
            match self {
                Entry::Sync { local, remote } => {
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
                Self::Sync { local, .. } => Some(local),
            }
        }

        pub fn into_remote_metadata(self) -> Option<super::Metadata> {
            match self {
                Self::Local(..) => None,
                Self::Remote(metadata) => Some(metadata),
                Self::Sync { remote, .. } => Some(remote),
            }
        }

        pub fn is_local_only(&self) -> bool {
            matches!(self, Entry::Local(..))
        }

        pub fn is_remote_only(&self) -> bool {
            matches!(self, Entry::Remote(..))
        }

        pub fn is_sync(&self) -> bool {
            matches!(self, Entry::Sync { .. })
        }

        pub fn is_safe_dir(&self) -> bool {
            match self {
                Self::Local(md) if md.is_dir() => true,
                Self::Remote(md) if md.is_dir() => true,
                Self::Sync { local, remote } if local.is_dir() && remote.is_dir() => true,
                _ => false,
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Node {
        entry: Entry,
        children: Vec<String>,
        conflict: Option<ConflictTy>,
        children_conflict_count: usize,
    }

    impl Node {
        pub fn new(entry: Entry, children: Vec<String>) -> Self {
            Self {
                entry,
                children,
                conflict: None,
                children_conflict_count: 0,
            }
        }

        pub fn entry(&self) -> &Entry {
            &self.entry
        }

        pub fn entry_mut(&mut self) -> &mut Entry {
            &mut self.entry
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

        pub fn is_sync(&self) -> bool {
            self.entry.is_sync()
        }

        pub fn conflict(&self) -> Option<ConflictTy> {
            self.conflict
        }

        pub fn set_conflict(&mut self, conflict: Option<ConflictTy>) {
            self.conflict = conflict;
        }

        pub fn has_conflict(&self) -> bool {
            self.conflict.is_some()
        }

        pub fn children_conflict_count(&self) -> usize {
            self.children_conflict_count
        }

        pub fn children_have_conflicts(&self) -> bool {
            self.children_conflict_count > 0
        }

        pub fn add_children_conflicts(&mut self, cc: isize) {
            let new_count = self.children_conflict_count as isize + cc;
            assert!(new_count >= 0);
            self.children_conflict_count = new_count as usize;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    CopyRemoteToLocal(PathBuf),
    CopyLocalToRemote(PathBuf),
    ReplaceLocalByRemote(PathBuf),
    ReplaceRemoteByLocal(PathBuf),
    Delete(PathBuf, Location),
}

#[tarpc::service]
pub trait Fsync {
    async fn conflict(path: PathBuf) -> crate::Result<Option<Conflict>>;
    async fn conflicts(first: Option<PathBuf>, max_len: u32) -> crate::Result<Vec<Conflict>>;
    async fn entry(path: PathBuf) -> crate::Result<Option<tree::Node>>;
    async fn operate(operation: Operation) -> crate::Result<()>;
}
