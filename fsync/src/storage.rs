use std::{sync::Arc, time::SystemTime};

use futures::future::BoxFuture;
use tokio::sync::mpsc::Sender;

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryType {
    Directory,
    Regular {
        size: u64,
        mtime: SystemTime,
    },
    Symlink {
        target: String,
        size: u64,
        mtime: SystemTime,
    },
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    id: String,
    path: String,
    typ: EntryType,
}

impl Entry {
    pub fn new(id: String, path: String, typ: EntryType) -> Entry {
        Entry { id, path, typ }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn path(&self) -> &str {
        &self.path
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

    pub fn mtime(&self) -> Option<SystemTime> {
        match self.typ {
            EntryType::Regular { mtime, .. } => Some(mtime),
            EntryType::Symlink { mtime, .. } => Some(mtime),
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

pub trait Storage: Send + Sync + 'static {
    fn entries(
        &self,
        dir_id: Option<&str>,
    ) -> impl std::future::Future<Output = Result<impl Iterator<Item = Result<Entry>> + Send>> + Send;

    fn discover(
        self: Arc<Self>,
        dir_id: Option<&str>,
        depth: Option<u32>,
        tx: Sender<Result<Entry>>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if let Some(0) = depth {
                return Ok(());
            }

            let entries = self.entries(dir_id).await?;
            for entry in entries {
                let dir_id = match &entry {
                    Ok(Entry {
                        id,
                        typ: EntryType::Directory,
                        ..
                    }) => Some(id.clone()),
                    _ => None,
                };

                tx.send(entry).await.unwrap();

                if let Some(dir_id) = dir_id {
                    let tx = tx.clone();
                    let this = self.clone();
                    tokio::spawn(async move {
                        this.discover(Some(&dir_id), depth.map(|depth| depth - 1), tx)
                            .await
                            .unwrap();
                    });
                }
            }
            Ok(())
        })
    }
}
