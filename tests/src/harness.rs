#![allow(dead_code)]

use std::sync::Arc;

use crate::utils;
use fsync::{
    path::Path,
    stat,
    tree::{Entry, EntryNode},
    Location, Metadata, StorageLoc,
};
use fsyncd::{service::Service, storage::Storage};

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

    pub async fn entry_node<P: AsRef<Path>>(&self, path: P) -> fsync::Result<Option<EntryNode>> {
        self.service.entry_node(path.as_ref()).await
    }

    pub async fn operate(&self, operation: fsync::Operation) -> fsync::Result<fsync::Progress> {
        self.service.clone().operate(operation).await
    }

    pub async fn metadata<P: AsRef<Path>>(
        &self,
        path: P,
        loc: StorageLoc,
    ) -> fsync::Result<Option<Metadata>> {
        let e = self.service.entry_node(path.as_ref()).await?;
        Ok(e.map(|node| node.into_entry().into_metadata(loc)).flatten())
    }

    pub async fn has_dir<P: AsRef<Path>>(
        &self,
        path: P,
        loc: fsync::Location,
    ) -> fsync::Result<bool> {
        use fsync::Metadata::Directory;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await?
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
            ) => Ok(true),
            _ => Ok(false),
        }
    }

    pub async fn has_file<P: AsRef<Path>>(&self, path: P, loc: Location) -> fsync::Result<bool> {
        use fsync::Metadata::Regular;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await?
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
            ) => Ok(true),
            _ => Ok(false),
        }
    }

    pub async fn file_content<P: AsRef<Path>>(
        &self,
        path: P,
        loc: StorageLoc,
    ) -> fsync::Result<String> {
        match loc {
            StorageLoc::Local => {
                let r = self
                    .local()
                    .read_file(path.as_ref().to_owned(), None)
                    .await?;
                Ok(utils::file_content(r).await?)
            }
            StorageLoc::Remote => {
                let r = self
                    .remote()
                    .read_file(path.as_ref().to_owned(), None)
                    .await?;
                Ok(utils::file_content(r).await?)
            }
        }
    }

    pub async fn tree_stats<P: AsRef<Path>>(&self, path: P) -> fsync::Result<Option<stat::Tree>> {
        let stats = self.entry_node(path).await?.map(|n| n.stats());
        Ok(stats)
    }
}

impl<L, R> Harness<L, R>
where
    L: Storage,
    R: Storage,
{
    pub async fn local_metadata<P: AsRef<Path>>(&self, path: P) -> fsync::Result<Option<Metadata>> {
        self.metadata(path, fsync::StorageLoc::Local).await
    }

    pub async fn has_local_dir<P: AsRef<Path>>(&self, path: P) -> fsync::Result<bool> {
        self.has_dir(path, Location::Local).await
    }

    pub async fn has_local_file<P: AsRef<Path>>(&self, path: P) -> fsync::Result<bool> {
        self.has_file(path, Location::Local).await
    }

    pub async fn local_file_content<P: AsRef<Path>>(&self, path: P) -> fsync::Result<String> {
        self.file_content(path, StorageLoc::Local).await
    }

    pub async fn remote_metadata<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> fsync::Result<Option<Metadata>> {
        self.metadata(path, fsync::StorageLoc::Remote).await
    }

    pub async fn has_remote_dir<P: AsRef<Path>>(&self, path: P) -> fsync::Result<bool> {
        self.has_dir(path, Location::Remote).await
    }

    pub async fn has_remote_file<P: AsRef<Path>>(&self, path: P) -> fsync::Result<bool> {
        self.has_file(path, Location::Remote).await
    }

    pub async fn remote_file_content<P: AsRef<Path>>(&self, path: P) -> fsync::Result<String> {
        self.file_content(path, StorageLoc::Remote).await
    }
}

impl<L, R> Harness<L, R>
where
    L: Storage,
    R: Storage,
{
    pub async fn has_sync_dir<P: AsRef<Path>>(&self, path: P) -> fsync::Result<bool> {
        use fsync::Metadata::Directory;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await?
            .map(|node| node.into_entry());
        match e {
            Some(Entry::Sync {
                local: Directory { .. },
                remote: Directory { .. },
                ..
            }) => Ok(true),
            _ => Ok(false),
        }
    }

    pub async fn has_sync_dir_no_conflict<P: AsRef<Path>>(&self, path: P) -> fsync::Result<bool> {
        use fsync::Metadata::Directory;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await?
            .map(|node| node.into_entry());
        match e {
            Some(Entry::Sync {
                local: Directory { .. },
                remote: Directory { .. },
                conflict: None,
            }) => Ok(true),
            _ => Ok(false),
        }
    }

    pub async fn has_sync_file<P: AsRef<Path>>(&self, path: P) -> fsync::Result<bool> {
        use fsync::Metadata::Regular;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await?
            .map(|node| node.into_entry());
        match e {
            Some(Entry::Sync {
                local: Regular { .. },
                remote: Regular { .. },
                ..
            }) => Ok(true),
            _ => Ok(false),
        }
    }

    pub async fn has_sync_file_no_conflict<P: AsRef<Path>>(&self, path: P) -> fsync::Result<bool> {
        use fsync::Metadata::Regular;

        let e = self
            .service
            .entry_node(path.as_ref())
            .await?
            .map(|node| node.into_entry());
        match e {
            Some(Entry::Sync {
                local: Regular { .. },
                remote: Regular { .. },
                conflict: None,
            }) => Ok(true),
            _ => Ok(false),
        }
    }
}

impl<L, R> Harness<L, R>
where
    L: Storage,
    R: Storage,
{
    pub async fn assert_dir<P: AsRef<Path>>(&self, path: P, loc: Location) {
        let path = path.as_ref();
        assert!(
            self.has_dir(path, loc).await.unwrap(),
            "no such directory in {loc:?}: {path}"
        );
    }

    pub async fn assert_file<P: AsRef<Path>>(&self, path: P, loc: Location) {
        let path = path.as_ref();
        assert!(
            self.has_file(path, loc).await.unwrap(),
            "no such file in {loc:?}: {path}"
        );
    }

    pub async fn assert_file_with_content<P: AsRef<Path>>(
        &self,
        path: P,
        content: &str,
        loc: Location,
    ) {
        let path = path.as_ref();
        self.assert_file(path, loc).await;
        match loc {
            Location::Local => assert_eq!(self.local_file_content(path).await.unwrap(), content),
            Location::Remote => assert_eq!(self.remote_file_content(path).await.unwrap(), content),
            Location::Both => {
                assert_eq!(self.local_file_content(path).await.unwrap(), content);
                assert_eq!(self.remote_file_content(path).await.unwrap(), content);
            }
        }
    }

    pub async fn assert_file_with_path_content<P: AsRef<Path>>(&self, path: P, loc: Location) {
        let path = path.as_ref();
        self.assert_file_with_content(path, path.as_str(), loc)
            .await;
    }

    pub async fn assert_local_dir<P: AsRef<Path>>(&self, path: P) {
        self.assert_dir(path, Location::Local).await;
    }

    pub async fn assert_local_file<P: AsRef<Path>>(&self, path: P) {
        self.assert_file(path, Location::Local).await;
    }

    pub async fn assert_local_file_with_content<P: AsRef<Path>>(&self, path: P, content: &str) {
        self.assert_file_with_content(path, content, Location::Local)
            .await;
    }

    pub async fn assert_local_file_with_path_content<P: AsRef<Path>>(&self, path: P) {
        self.assert_file_with_path_content(path, Location::Local)
            .await;
    }

    pub async fn assert_remote_dir<P: AsRef<Path>>(&self, path: P) {
        self.assert_dir(path, Location::Remote).await;
    }

    pub async fn assert_remote_file<P: AsRef<Path>>(&self, path: P) {
        self.assert_file(path, Location::Remote).await;
    }

    pub async fn assert_remote_file_with_content<P: AsRef<Path>>(&self, path: P, content: &str) {
        self.assert_file_with_content(path, content, Location::Remote)
            .await;
    }

    pub async fn assert_remote_file_with_path_content<P: AsRef<Path>>(&self, path: P) {
        self.assert_file_with_path_content(path, Location::Remote)
            .await;
    }

    pub async fn assert_sync_dir<P: AsRef<Path>>(&self, path: P) {
        let path = path.as_ref();
        assert!(
            self.has_sync_dir(path).await.unwrap(),
            "no such sync directory: {path}"
        );
    }

    pub async fn assert_sync_file<P: AsRef<Path>>(&self, path: P) {
        let path = path.as_ref();
        assert!(
            self.has_sync_file(path).await.unwrap(),
            "no such sync file: {path}"
        );
    }

    pub async fn assert_sync_file_with_content<P: AsRef<Path>>(&self, path: P, content: &str) {
        self.assert_file_with_content(path, content, Location::Both)
            .await;
    }

    pub async fn assert_sync_file_with_path_content<P: AsRef<Path>>(&self, path: P) {
        self.assert_file_with_path_content(path, Location::Both)
            .await;
    }

    pub async fn assert_tree_stats<P: AsRef<Path>>(&self, path: P, expected: &stat::Tree) {
        let actual = self
            .tree_stats(path)
            .await
            .unwrap()
            .expect("No such tree node: {path}");
        assert_eq!(*expected, actual);
    }
}
