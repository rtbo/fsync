use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use futures::Stream;

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryType {
    Directory,
    Regular {
        size: u64,
        mtime: Option<DateTime<Utc>>,
    },
    Symlink {
        target: String,
        size: u64,
        mtime: Option<DateTime<Utc>>,
    },
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    id: String,
    path: Utf8PathBuf,
    typ: EntryType,
}

impl Entry {
    pub fn new(id: String, path: Utf8PathBuf, typ: EntryType) -> Entry {
        Entry { id, path, typ }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn path(&self) -> &Utf8Path {
        &self.path
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
            EntryType::Regular { mtime, .. } => mtime,
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

#[derive(Debug, Copy, Clone)]
pub struct PathId<'a> {
    pub id: &'a str,
    pub path: &'a Utf8Path,
}

pub trait Storage {
    fn entries(&self, dir_id: Option<PathId>) -> impl Stream<Item = Result<Entry>> + Send;
}
