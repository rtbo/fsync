#![cfg(test)]

use std::sync::{Arc, Once};

use dataset::Dataset;
use fsync::{path::Path, Metadata};
use fsyncd::{
    service::Service,
    storage::{
        cache::{CachePersist, CacheStorage},
        Storage,
    },
};

//mod config;
mod dataset;
mod utils;
mod stubs {
    //pub mod drive;
    pub mod fs;
    pub mod id;
}
mod tests;

use futures::FutureExt;
use stubs::{fs, id};

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

    pub async fn local_metadata(&self, path: &Path) -> anyhow::Result<Option<Metadata>> {
        let e = self.service.entry_node(path).await?;
        Ok(e.map(|node| node.into_entry().into_local_metadata())
            .flatten())
    }

    pub async fn remote_metadata(&self, path: &Path) -> anyhow::Result<Option<Metadata>> {
        let e = self.service.entry_node(path).await?;
        Ok(e.map(|node| node.into_entry().into_remote_metadata())
            .flatten())
    }

    pub async fn local_file_content(&self, path: &Path) -> anyhow::Result<String> {
        let r = self.local().read_file(path.to_owned(), None).await?;
        let c = utils::file_content(r).await?;
        Ok(c)
    }

    pub async fn remote_file_content(&self, path: &Path) -> anyhow::Result<String> {
        let r = self.remote().read_file(path.to_owned(), None).await?;
        let c = utils::file_content(r).await?;
        Ok(c)
    }
}

type CacheHarness = Harness<fs::Stub, CacheStorage<id::Stub>>;

static LOG_INIT: Once = Once::new();

async fn harness() -> CacheHarness {
    harness_with(Dataset::default()).await
}

async fn harness_with(dataset: Dataset) -> CacheHarness {
    LOG_INIT.call_once(env_logger::init);

    let now = dataset.mtime_ref;

    let dst = utils::temp_path(Some("fsync-fs"), None);
    tokio::fs::create_dir(&dst).await.unwrap();

    let local_root = dst.join("local");
    let local = fs::Stub::new(&local_root, &dataset.local, now);

    let remote_root = dst.join("remote");
    let remote = id::Stub::new(&remote_root, &dataset.remote, now)
        .then(|remote| async { CacheStorage::new(remote.unwrap(), CachePersist::Memory).await });

    let (local, remote) = tokio::try_join!(local, remote).unwrap();

    let service = Arc::new(Service::new(local, remote).await.unwrap());

    Harness { service }
}
