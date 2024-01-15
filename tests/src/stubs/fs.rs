use fsync::path::FsPath;
use fsyncd::storage;
use fsyncd::storage::fs::FileSystem;
use futures::{Future, Stream};
use tokio::{fs, io};

use crate::utils;

#[derive(Debug, Clone)]
pub struct Stub {
    inner: FileSystem,
}

impl Stub {
    pub async fn new(prefix: &str, path: &FsPath) -> anyhow::Result<Self> {
        let td = utils::temp_path(Some(prefix), None);
        println!("copying {path} to {td}");
        utils::copy_dir_all(path, &td).await?;
        let inner = FileSystem::new(&td)?;
        Ok(Self { inner })
    }
    fn root(&self) -> &FsPath {
        self.inner.root()
    }
}

impl storage::DirEntries for Stub {
    fn dir_entries(
        &self,
        parent_path: fsync::path::PathBuf,
    ) -> impl Stream<Item = anyhow::Result<fsync::Metadata>> + Send {
        self.inner.dir_entries(parent_path)
    }
}

impl storage::ReadFile for Stub {
    fn read_file(
        &self,
        path: fsync::path::PathBuf,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead + Send>> + Send {
        self.inner.read_file(path)
    }
}

impl storage::MkDir for Stub {
    fn mkdir(
        &self,
        path: &fsync::path::Path,
        parents: bool,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        self.inner.mkdir(path, parents)
    }
}

impl storage::CreateFile for Stub {
    fn create_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<fsync::Metadata>> + Send {
        self.inner.create_file(metadata, data)
    }
}

impl fsyncd::Shutdown for Stub {
    async fn shutdown(&self) -> anyhow::Result<()> {
        let _ = fs::remove_dir_all(self.root()).await;
        Ok(())
    }
}

impl storage::Storage for Stub {}
