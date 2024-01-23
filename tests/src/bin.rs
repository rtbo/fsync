#![cfg(test)]

use std::sync::Once;

use fsync::{
    path::{FsPath, Path},
    Metadata,
};
use fsyncd::{
    service::Service,
    storage::{
        cache::{CachePersist, CacheStorage},
        Storage,
    },
};

//mod config;
mod utils;
mod stubs {
    //pub mod drive;
    pub mod fs;
    pub mod id;
}
mod tests;

use stubs::{fs, id};

pub struct Harness<L, R> {
    pub service: Service<L, R>,
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
        let e = self.service.entry(path).await?;
        Ok(e.map(|node| node.into_entry().into_local_metadata())
            .flatten())
    }

    pub async fn remote_metadata(&self, path: &Path) -> anyhow::Result<Option<Metadata>> {
        let e = self.service.entry(path).await?;
        Ok(e.map(|node| node.into_entry().into_remote_metadata())
            .flatten())
    }

    pub async fn local_file_content(&self, path: &Path) -> anyhow::Result<String> {
        let r = self.local().read_file(path.to_owned()).await?;
        let c = utils::file_content(r).await?;
        Ok(c)
    }

    pub async fn remote_file_content(&self, path: &Path) -> anyhow::Result<String> {
        let r = self.remote().read_file(path.to_owned()).await?;
        let c = utils::file_content(r).await?;
        Ok(c)
    }
}

type CacheHarness = Harness<fs::Stub, CacheStorage<id::Stub>>;

static LOG_INIT: Once = Once::new();

async fn harness() -> CacheHarness {
    LOG_INIT.call_once(env_logger::init);

    let dir = FsPath::new(env!("CARGO_MANIFEST_DIR"));

    let local_dir = dir.join("local");
    let local = fs::Stub::new(&local_dir);

    let remote_dir = dir.join("remote");
    let remote = id::Stub::new(&remote_dir).await.unwrap();
    let remote = CacheStorage::new(remote, CachePersist::Memory);

    let (local, remote) = tokio::try_join!(local, remote).unwrap();

    let service = Service::new(local, remote).await.unwrap();

    Harness { service }
}
