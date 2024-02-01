use fsync::path::{FsPath, Path};
use fsyncd::{storage, storage::fs::FileSystem};
use futures::{Future, Stream};
use tokio::{fs, io};

use crate::utils;

#[derive(Debug, Clone)]
pub struct Stub {
    inner: FileSystem,
}

impl Stub {
    pub async fn new(src: &FsPath) -> anyhow::Result<Self> {
        let dst = utils::temp_path(Some("fsync-fs"), None);
        utils::copy_dir_all(src, &dst).await?;
        let inner = FileSystem::new(&dst)?;
        Ok(Self { inner })
    }

    fn root(&self) -> &FsPath {
        self.inner.root()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.inner.root()).unwrap();
    }
}

impl storage::DirEntries for Stub {
    fn dir_entries(
        &self,
        parent_path: &Path,
    ) -> impl Stream<Item = fsync::Result<fsync::Metadata>> + Send {
        self.inner.dir_entries(parent_path)
    }
}

impl storage::ReadFile for Stub {
    fn read_file(
        &self,
        path: fsync::path::PathBuf,
    ) -> impl Future<Output = fsync::Result<impl io::AsyncRead + Send>> + Send {
        self.inner.read_file(path)
    }
}

impl storage::MkDir for Stub {
    fn mkdir(
        &self,
        path: &fsync::path::Path,
        parents: bool,
    ) -> impl Future<Output = fsync::Result<()>> + Send {
        self.inner.mkdir(path, parents)
    }
}

impl storage::CreateFile for Stub {
    fn create_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = fsync::Result<fsync::Metadata>> + Send {
        self.inner.create_file(metadata, data)
    }
}

impl storage::WriteFile for Stub {
    fn write_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = fsync::Result<fsync::Metadata>> + Send {
        self.inner.write_file(metadata, data)
    }
}

impl storage::Delete for Stub {
    fn delete(&self, path: &Path) -> impl Future<Output = fsync::Result<()>> + Send {
        self.inner.delete(path)
    }
}

impl fsyncd::Shutdown for Stub {
    async fn shutdown(&self) -> anyhow::Result<()> {
        let _ = fs::remove_dir_all(self.root()).await;
        Ok(())
    }
}

impl storage::Storage for Stub {}
