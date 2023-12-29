use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryType {
    Directory,
    Regular {
        size: u64,
        mtime: DateTime<Utc>,
    },
    Symlink {
        target: String,
        size: u64,
        mtime: Option<DateTime<Utc>>,
    },
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    id: String,
    path: Utf8PathBuf,
    typ: EntryType,
}

impl Entry {
    pub fn root() -> Entry {
        Entry {
            id: "".to_string(),
            path: Utf8PathBuf::from(""),
            typ: EntryType::Directory,
        }
    }

    pub fn new(id: String, path: Utf8PathBuf, typ: EntryType) -> Entry {
        Entry { id, path, typ }
    }

    pub fn path_id(&self) -> PathId<'_> {
        PathId {
            id: &self.id,
            path: &self.path,
        }
    }

    pub fn path_id_buf(&self) -> PathIdBuf {
        PathIdBuf {
            id: self.id.to_owned(),
            path: self.path.to_owned(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub fn path_or_root(&self) -> &str {
        if self.path.as_str().is_empty() {
            "(root)"
        } else {
            self.path().as_str()
        }
    }

    pub fn name(&self) -> &str {
        self.path.file_name().unwrap_or("")
    }

    pub fn typ(&self) -> &EntryType {
        &self.typ
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.typ, EntryType::Directory)
    }

    pub fn is_file(&self) -> bool {
        matches!(self.typ, EntryType::Regular { .. })
    }

    pub fn is_symlink(&self) -> bool {
        matches!(self.typ, EntryType::Symlink { .. })
    }

    pub fn is_special(&self) -> bool {
        matches!(self.typ, EntryType::Special)
    }

    pub fn size(&self) -> Option<u64> {
        match self.typ {
            EntryType::Regular { size, .. } => Some(size),
            EntryType::Symlink { size, .. } => Some(size),
            _ => None,
        }
    }

    pub fn mtime(&self) -> Option<DateTime<Utc>> {
        match self.typ {
            EntryType::Regular { mtime, .. } => Some(mtime),
            EntryType::Symlink { mtime, .. } => mtime,
            _ => None,
        }
    }

    pub fn symlink_target(&self) -> Option<&str> {
        match &self.typ {
            EntryType::Symlink { target, .. } => Some(target),
            _ => None,
        }
    }
}

impl Default for Entry {
    fn default() -> Self {
        Entry::root()
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

pub trait DirEntries {
    fn dir_entries(
        &self,
        parent_path_id: Option<PathId>,
    ) -> impl Stream<Item = Result<Entry>> + Send;
}

pub trait ReadFile {
    async fn read_file(&self, path_id: PathId) -> Result<impl tokio::io::AsyncRead>;
}

pub trait CreateFile {
    async fn create_file(&self, metadata: &Entry, data: impl tokio::io::AsyncRead) -> Result<()>;
}

pub trait Storage: Clone + DirEntries + ReadFile + CreateFile + Send + Sync + 'static {}
