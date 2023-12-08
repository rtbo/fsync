use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use futures::{Future, Stream};
use tokio::sync::mpsc::{self, Sender};
use tokio_stream::wrappers::ReceiverStream;

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
        match self.typ {
            EntryType::Directory => true,
            _ => false,
        }
    }

    pub fn is_file(&self) -> bool {
        match self.typ {
            EntryType::Regular { .. } => true,
            _ => false,
        }
    }

    pub fn is_symlink(&self) -> bool {
        match self.typ {
            EntryType::Symlink { .. } => true,
            _ => false,
        }
    }

    pub fn is_special(&self) -> bool {
        match self.typ {
            EntryType::Special => true,
            _ => false,
        }
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
    pub path: &'a str,
}

pub trait Storage {
    fn entries(
        &self,
        dir_id: Option<PathId>,
        sender: Sender<Entry>,
    ) -> impl Future<Output = Result<()>> + Send;

    fn entries2(
        &self,
        dir_id: Option<PathId>,
    ) -> impl Future<Output = impl Stream<Item = Result<Entry>> + Send> + Send;

    fn entries_stream<'a>(
        &self,
        dir_id: Option<PathId<'a>>,
    ) -> impl Future<Output = Result<impl Stream<Item = Entry> + Send>> + Send
    where
        Self: Sync,
    {
        async move {
            let (snd, rcv) = mpsc::channel(512);
            self.entries(dir_id, snd).await?;
            Ok(ReceiverStream::new(rcv))
        }
    }
}
