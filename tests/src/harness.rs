#![allow(dead_code)]

use std::sync::Arc;

use fsync::{
    path::Path,
    stat,
    tree::{Entry, EntryNode},
    Location, Metadata, StorageLoc,
};
use fsyncd::{service::Service, storage::Storage};

use crate::utils;

#[derive(Clone)]
pub struct Harness<L, R> {
    pub service: Arc<Service<L, R>>,
}

impl<L, R> Harness<L, R>
where
    L: Storage,
    R: Storage,
{
    pub fn local(&self) -> &L {
        self.service.local()
    }

    pub fn remote(&self) -> &R {
        self.service.remote()
    }

    pub async fn entry_node<P: AsRef<Path>>(&self, path: P) -> Option<EntryNode> {
        self.service
            .entry_node(path.as_ref())
            .await
            .expect("Should not fail")
    }

    pub async fn operate(&self, operation: fsync::Operation) -> fsync::Progress {
        self.service
            .clone()
            .operate(operation)
            .await
            .expect("Should not fail")
    }

    pub async fn metadata<P: AsRef<Path>>(&self, path: P, loc: StorageLoc) -> Option<Metadata> {
        let e = self
            .service
            .entry_node(path.as_ref())
            .await
            .expect("Should not fail");
        e.map(|node| node.into_entry().into_metadata(loc)).flatten()
    }

    pub async fn has_dir<P: AsRef<Path>>(&self, path: P, loc: fsync::Location) -> bool {
        use fsync::Metadata::Directory;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await
            .expect("Should not fail")
            .map(|node| node.into_entry());
        match (loc, e) {
            (
                Location::Both,
                Some(Entry::Sync {
                    local: Directory { .. },
                    remote: Directory { .. },
                    ..
                }),
            )
            | (Location::Local, Some(Entry::Local(Directory { .. })))
            | (
                Location::Local,
                Some(Entry::Sync {
                    local: Directory { .. },
                    ..
                }),
            )
            | (Location::Remote, Some(Entry::Remote(Directory { .. })))
            | (
                Location::Remote,
                Some(Entry::Sync {
                    remote: Directory { .. },
                    ..
                }),
            ) => true,
            _ => false,
        }
    }

    pub async fn has_file<P: AsRef<Path>>(&self, path: P, loc: Location) -> bool {
        use fsync::Metadata::Regular;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await
            .expect("Should not fail")
            .map(|node| node.into_entry());
        match (loc, e) {
            (
                Location::Both,
                Some(Entry::Sync {
                    local: Regular { .. },
                    remote: Regular { .. },
                    ..
                }),
            )
            | (Location::Local, Some(Entry::Local(Regular { .. })))
            | (
                Location::Local,
                Some(Entry::Sync {
                    local: Regular { .. },
                    ..
                }),
            )
            | (Location::Remote, Some(Entry::Remote(Regular { .. })))
            | (
                Location::Remote,
                Some(Entry::Sync {
                    remote: Regular { .. },
                    ..
                }),
            ) => true,
            _ => false,
        }
    }

    pub async fn file_content<P: AsRef<Path>>(&self, path: P, loc: StorageLoc) -> Option<String> {
        if !self.has_file(path.as_ref(), loc.into()).await {
            return None;
        }
        match loc {
            StorageLoc::Local => {
                let r = self
                    .local()
                    .read_file(path.as_ref().to_owned(), None)
                    .await
                    .unwrap();
                Some(utils::file_content(r).await.expect("Should not fail"))
            }
            StorageLoc::Remote => {
                let r = self
                    .remote()
                    .read_file(path.as_ref().to_owned(), None)
                    .await
                    .unwrap();
                Some(utils::file_content(r).await.expect("Should not fail"))
            }
        }
    }

    pub async fn has_file_with_content<P: AsRef<Path>>(
        &self,
        path: P,
        content: &str,
        loc: Location,
    ) -> bool {
        let path = path.as_ref();
        if !self.has_file(path, loc.into()).await {
            return false;
        }
        match loc {
            Location::Local => self.file_content(path, StorageLoc::Local).await.unwrap() == content,
            Location::Remote => {
                self.file_content(path, StorageLoc::Remote).await.unwrap() == content
            }
            Location::Both => {
                self.file_content(path, StorageLoc::Local).await.unwrap() == content
                    && self.file_content(path, StorageLoc::Remote).await.unwrap() == content
            }
        }
    }

    pub async fn has_file_with_path_content<P: AsRef<Path>>(&self, path: P, loc: Location) -> bool {
        let path = path.as_ref();
        self.has_file_with_content(path, path.as_str(), loc).await
    }

    pub async fn tree_stats<P: AsRef<Path>>(&self, path: P) -> Option<stat::Tree> {
        self.entry_node(path).await.map(|n| n.stats())
    }
}

impl<L, R> Harness<L, R>
where
    L: Storage,
    R: Storage,
{
    pub async fn local_metadata<P: AsRef<Path>>(&self, path: P) -> Option<Metadata> {
        self.metadata(path, fsync::StorageLoc::Local).await
    }

    pub async fn has_local_dir<P: AsRef<Path>>(&self, path: P) -> bool {
        self.has_dir(path, Location::Local).await
    }

    pub async fn has_local_file<P: AsRef<Path>>(&self, path: P) -> bool {
        self.has_file(path, Location::Local).await
    }

    pub async fn local_file_content<P: AsRef<Path>>(&self, path: P) -> Option<String> {
        self.file_content(path, StorageLoc::Local).await
    }

    pub async fn has_local_file_with_content<P: AsRef<Path>>(
        &self,
        path: P,
        content: &str,
    ) -> bool {
        self.has_file_with_content(path, content, Location::Local)
            .await
    }

    pub async fn has_local_file_with_path_content<P: AsRef<Path>>(&self, path: P) -> bool {
        self.has_file_with_path_content(path, Location::Local).await
    }

    pub async fn remote_metadata<P: AsRef<Path>>(&self, path: P) -> Option<Metadata> {
        self.metadata(path, fsync::StorageLoc::Remote).await
    }

    pub async fn has_remote_dir<P: AsRef<Path>>(&self, path: P) -> bool {
        self.has_dir(path, Location::Remote).await
    }

    pub async fn has_remote_file<P: AsRef<Path>>(&self, path: P) -> bool {
        self.has_file(path, Location::Remote).await
    }

    pub async fn remote_file_content<P: AsRef<Path>>(&self, path: P) -> Option<String> {
        self.file_content(path, StorageLoc::Remote).await
    }

    pub async fn has_remote_file_with_content<P: AsRef<Path>>(
        &self,
        path: P,
        content: &str,
    ) -> bool {
        self.has_file_with_content(path, content, Location::Remote)
            .await
    }

    pub async fn has_remote_file_with_path_content<P: AsRef<Path>>(&self, path: P) -> bool {
        self.has_file_with_path_content(path, Location::Remote)
            .await
    }
}

impl<L, R> Harness<L, R>
where
    L: Storage,
    R: Storage,
{
    pub async fn has_sync_dir<P: AsRef<Path>>(&self, path: P) -> bool {
        use fsync::Metadata::Directory;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await
            .expect("Should not fail")
            .map(|node| node.into_entry());
        match e {
            Some(Entry::Sync {
                local: Directory { .. },
                remote: Directory { .. },
                ..
            }) => true,
            _ => false,
        }
    }

    pub async fn has_sync_dir_no_conflict<P: AsRef<Path>>(&self, path: P) -> bool {
        use fsync::Metadata::Directory;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await
            .expect("Should not fail")
            .map(|node| node.into_entry());
        match e {
            Some(Entry::Sync {
                local: Directory { .. },
                remote: Directory { .. },
                conflict: None,
            }) => true,
            _ => false,
        }
    }

    pub async fn has_sync_file<P: AsRef<Path>>(&self, path: P) -> bool {
        use fsync::Metadata::Regular;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await
            .expect("Should not fail")
            .map(|node| node.into_entry());
        match e {
            Some(Entry::Sync {
                local: Regular { .. },
                remote: Regular { .. },
                ..
            }) => true,
            _ => false,
        }
    }

    pub async fn has_sync_file_no_conflict<P: AsRef<Path>>(&self, path: P) -> bool {
        use fsync::Metadata::Regular;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await
            .expect("Should not fail")
            .map(|node| node.into_entry());
        match e {
            Some(Entry::Sync {
                local: Regular { .. },
                remote: Regular { .. },
                conflict: None,
            }) => true,
            _ => false,
        }
    }

    pub async fn has_sync_file_with_content<P: AsRef<Path>>(&self, path: P, content: &str) -> bool {
        let path = path.as_ref();
        if !self.has_sync_file(path).await {
            return false;
        }

        self.file_content(path, StorageLoc::Local).await.unwrap() == content
            && self.file_content(path, StorageLoc::Remote).await.unwrap() == content
    }

    pub async fn has_sync_file_with_path_content<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path.as_ref();
        self.has_sync_file_with_content(path, path.as_str()).await
    }
}
