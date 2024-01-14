#![cfg(test)]

use std::sync::Arc;

use fsync::{
    path::{FsPath, FsPathBuf, Path, PathBuf},
    Fsync,
};
use fsyncd::{
    service::Service,
    storage::{self, fs::FileSystem, Storage},
};
use futures::prelude::*;
use futures::{future::BoxFuture, stream::AbortHandle};
use rand::{distributions::Alphanumeric, Rng};
use tarpc::context::current;
use tokio::{
    fs::{self},
    io::{self, AsyncReadExt},
};

fn temp_path(prefix: Option<&str>, ext: Option<&str>) -> FsPathBuf {
    let mut filename = String::new();
    if let Some(prefix) = prefix {
        filename.push_str(prefix);
        filename.push('-');
    }
    let rnd: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();
    filename.push_str(&rnd);
    if let Some(ext) = ext {
        filename.push('.');
        filename.push_str(ext);
    }
    let mut p = std::env::temp_dir();
    p.push(filename);
    p.try_into().unwrap()
}

fn copy_dir_all<'a>(
    src: impl AsRef<FsPath>,
    dst: impl AsRef<FsPath>,
) -> BoxFuture<'a, anyhow::Result<()>> {
    let src = src.as_ref().as_std_path().to_owned();
    let dst = dst.as_ref().as_std_path().to_owned();

    Box::pin(async move {
        fs::create_dir_all(&dst).await?;
        let mut entries = fs::read_dir(&src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let ty = entry.file_type().await?;
            if ty.is_dir() {
                let src: FsPathBuf = entry.path().try_into()?;
                let dst: FsPathBuf = dst.join(entry.file_name()).try_into()?;
                copy_dir_all(&src, &dst).await?;
            } else {
                fs::copy(entry.path(), dst.join(entry.file_name())).await?;
            }
        }
        Ok(())
    })
}

#[derive(Debug, Clone)]
pub struct FsStub {
    inner: FileSystem,
}

impl FsStub {
    async fn new(prefix: &str, path: &FsPath) -> anyhow::Result<Self> {
        let td = temp_path(Some(prefix), None);
        println!("copying {path} to {td}");
        copy_dir_all(path, &td).await?;
        let inner = FileSystem::new(&td)?;
        Ok(Self { inner })
    }
    fn root(&self) -> &FsPath {
        self.inner.root()
    }
}

impl Drop for FsStub {
    fn drop(&mut self) {
        let root = self.root().to_owned();
        tokio::spawn(async move {
            let _ = fs::remove_dir_all(root).await;
        });
    }
}

impl storage::DirEntries for FsStub {
    fn dir_entries(
        &self,
        parent_path: fsync::path::PathBuf,
    ) -> impl Stream<Item = anyhow::Result<fsync::Metadata>> + Send {
        self.inner.dir_entries(parent_path)
    }
}

impl storage::ReadFile for FsStub {
    fn read_file(
        &self,
        path: fsync::path::PathBuf,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead + Send>> + Send {
        self.inner.read_file(path)
    }
}

impl storage::MkDir for FsStub {
    fn mkdir(
        &self,
        path: &fsync::path::Path,
        parents: bool,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        self.inner.mkdir(path, parents)
    }
}

impl storage::CreateFile for FsStub {
    fn create_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<fsync::Metadata>> + Send {
        self.inner.create_file(metadata, data)
    }
}

impl fsyncd::PersistCache for FsStub {}

impl Storage for FsStub {}

pub struct Harness<L, R> {
    pub service: Service<L, R>,
    pub local: Arc<L>,
    pub remote: Arc<R>,
}

impl<L, R> Harness<L, R>
where
    L: Storage,
    R: Storage,
{
    pub async fn local_file_content(&self, path: &Path) -> anyhow::Result<String> {
        let r = self.local.read_file(path.to_owned()).await?;
        let c = file_content(r).await?;
        Ok(c)
    }
}

pub async fn make_fs_harness() -> Harness<FsStub, FsStub> {
    let dir = FsPath::new(env!("CARGO_MANIFEST_DIR"));

    let local_dir = dir.join("local");
    let local = FsStub::new("local", &local_dir);

    let remote_dir = dir.join("remote");
    let remote = FsStub::new("remote", &remote_dir);

    let (local, remote) = tokio::try_join!(local, remote).unwrap();
    let local = Arc::new(local);
    let remote = Arc::new(remote);

    let (abort_handle, _abort_reg) = AbortHandle::new_pair();

    let service = Service::new_shared(local.clone(), remote.clone(), abort_handle)
        .await
        .unwrap();

    // {
    //     let service = service.clone();
    //     tokio::spawn(async move { service.start("test", abort_reg).await });
    // }

    Harness {
        service,
        local,
        remote,
    }
}

pub async fn file_content<R>(read: R) -> anyhow::Result<String>
where
    R: io::AsyncRead,
{
    tokio::pin!(read);
    let mut s = String::new();
    read.read_to_string(&mut s).await?;
    Ok(s)
}

#[tokio::test]
async fn test_copy_remote_to_local() -> anyhow::Result<()> {
    let harness = make_fs_harness().await;

    let path = PathBuf::from("/only-remote.txt");

    harness
        .service
        .clone()
        .copy_remote_to_local(current(), path.clone())
        .await
        .unwrap();

    let content = harness.local_file_content(&path).await?;
    assert_eq!(&content, path.as_str());
    Ok(())
}

#[tokio::test]
#[should_panic(expected = "No such entry in remote drive: '/not-a-file.txt'")]
async fn test_copy_remote_to_local_fail_missing() {
    let harness = make_fs_harness().await;

    let path = PathBuf::from("/not-a-file.txt");

    harness
        .service
        .copy_remote_to_local(current(), path.clone())
        .await
        .unwrap();
}

#[tokio::test]
#[should_panic(expected = "Expect an absolute path, got 'only-remote.txt'")]
async fn test_copy_remote_to_local_fail_relative() {
    let harness = make_fs_harness().await;

    let path = PathBuf::from("only-remote.txt");

    harness
        .service
        .copy_remote_to_local(current(), path.clone())
        .await
        .unwrap();
}
