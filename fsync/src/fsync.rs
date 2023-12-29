use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Metadata {
    Directory {
        id: String,
        path: Utf8PathBuf,
    },
    Regular {
        id: String,
        path: Utf8PathBuf,
        size: u64,
        mtime: DateTime<Utc>,
    },
    Symlink {
        id: String,
        path: Utf8PathBuf,
        target: String,
        size: u64,
        mtime: Option<DateTime<Utc>>,
    },
    Special {
        id: String,
        path: Utf8PathBuf,
    },
}

impl Metadata {
    pub fn root() -> Self {
        Self::Directory {
            id: Default::default(),
            path: Default::default(),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Directory { id, .. } => id,
            Self::Regular { id, .. } => id,
            Self::Symlink { id, .. } => id,
            Self::Special { id, .. } => id,
        }
    }

    pub fn path(&self) -> &Utf8Path {
        match self {
            Self::Directory { path, .. } => path,
            Self::Regular { path, .. } => path,
            Self::Symlink { path, .. } => path,
            Self::Special { path, .. } => path,
        }
    }

    pub fn path_id(&self) -> PathId<'_> {
        match self {
            Self::Directory { path, id, .. } => PathId { path, id },
            Self::Regular { path, id, .. } => PathId { path, id },
            Self::Symlink { path, id, .. } => PathId { path, id },
            Self::Special { path, id, .. } => PathId { path, id },
        }
    }

    pub fn path_id_buf(&self) -> PathIdBuf {
        self.path_id().to_path_id_buf()
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
    pub path: &'a Utf8Path,
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
    pub path: Utf8PathBuf,
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
    use camino::Utf8Path;
    use serde::{Deserialize, Serialize};
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
        fn with_remote(self, remote: super::Metadata) -> Self {
            match self {
                Entry::Local(local) => Entry::Both { local, remote },
                Entry::Remote(..) => Entry::Remote(remote),
                Entry::Both { local, .. } => Entry::Both { local, remote },
            }
        }

        fn with_local(self, local: super::Metadata) -> Self {
            match self {
                Entry::Remote(remote) => Entry::Both { local, remote },
                Entry::Local(..) => Entry::Local(local),
                Entry::Both { remote, .. } => Entry::Both { local, remote },
            }
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

        pub fn children(&self) -> &[String] {
            &self.children
        }

        pub fn path(&self) -> &Utf8Path {
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

        pub fn add_local(&mut self, local: super::Metadata) {
            use std::mem;
            let invalid: Entry = unsafe { mem::MaybeUninit::zeroed().assume_init() };
            let valid = mem::replace(&mut self.entry, invalid);
            self.entry = valid.with_local(local);
        }

        pub fn add_remote(&mut self, remote: super::Metadata) {
            use std::mem;
            let invalid: Entry = unsafe { mem::MaybeUninit::zeroed().assume_init() };
            let valid = mem::replace(&mut self.entry, invalid);
            self.entry = valid.with_remote(remote);
        }
    }
}

#[tarpc::service]
pub trait Fsync {
    async fn entry(path: Option<Utf8PathBuf>) -> Option<tree::Node>;
    async fn copy_remote_to_local(path: Utf8PathBuf) -> Result<(), String>;
}
