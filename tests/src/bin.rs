#![cfg(test)]

use fsync::path::{FsPath, Path, PathBuf};
use fsyncd::{service::Service, storage::{Storage, cache::{CacheStorage, CachePersist}}};

mod config;
mod utils;
pub mod stubs {
    pub mod id;
    pub mod drive;
    pub mod fs;
}

use stubs::{drive, fs, id};

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

    pub async fn local_file_content(&self, path: &Path) -> anyhow::Result<String> {
        let r = self.local().read_file(path.to_owned()).await?;
        let c = utils::file_content(r).await?;
        Ok(c)
    }
}

pub async fn make_fs_harness() -> Harness<fs::Stub, fs::Stub> {
    let dir = FsPath::new(env!("CARGO_MANIFEST_DIR"));

    let local_dir = dir.join("local");
    let local = fs::Stub::new(&local_dir);

    let remote_dir = dir.join("remote");
    let remote = fs::Stub::new(&remote_dir);

    let (local, remote) = tokio::try_join!(local, remote).unwrap();

    let service = Service::new(local, remote).await.unwrap();

    Harness { service }
}

async fn make_cache_harness() -> Harness<fs::Stub, CacheStorage<id::Stub>> {
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

async fn _make_drive_harness() -> Harness<fs::Stub, drive::Stub> {
    let dir = FsPath::new(env!("CARGO_MANIFEST_DIR"));

    let local_dir = dir.join("local");
    let local = fs::Stub::new(&local_dir);

    let remote_cache = dir.join("remote");
    let remote = drive::Stub::new(&remote_cache);

    let (local, remote) = tokio::try_join!(local, remote).unwrap();

    let service = Service::new(local, remote).await.unwrap();

    Harness { service }
}

#[tokio::test]
async fn test_copy_remote_to_local_cache() -> anyhow::Result<()> {
    let harness = make_cache_harness().await;

    let path = PathBuf::from("/only-remote.txt");

    harness.service.copy_remote_to_local(&path).await.unwrap();

    let content = harness.local_file_content(&path).await?;
    assert_eq!(&content, path.as_str());
    Ok(())
}

#[tokio::test]
async fn test_copy_remote_to_local() -> anyhow::Result<()> {
    let harness = make_fs_harness().await;

    let path = PathBuf::from("/only-remote.txt");

    harness.service.copy_remote_to_local(&path).await.unwrap();

    let content = harness.local_file_content(&path).await?;
    assert_eq!(&content, path.as_str());
    Ok(())
}

#[tokio::test]
#[should_panic(expected = "No such entry in remote drive: '/not-a-file.txt'")]
async fn test_copy_remote_to_local_fail_missing() {
    let harness = make_fs_harness().await;

    let path = PathBuf::from("/not-a-file.txt");

    harness.service.copy_remote_to_local(&path).await.unwrap();
}

#[tokio::test]
#[should_panic(expected = "Expect an absolute path, got 'only-remote.txt'")]
async fn test_copy_remote_to_local_fail_relative() {
    let harness = make_fs_harness().await;

    let path = PathBuf::from("only-remote.txt");

    harness.service.copy_remote_to_local(&path).await.unwrap();
}
