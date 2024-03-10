use std::time::SystemTime;

use fsync::path::{FsPath, Path};
use fsyncd::{
    storage::{self, fs::FileSystem},
    SharedProgress,
};
use futures::{Future, Stream};
use tokio::{fs, io};

use crate::dataset::{self, CreateDataset};

#[derive(Debug, Clone)]
pub struct Stub {
    inner: FileSystem,
}

impl Stub {
    pub async fn new(
        root: &FsPath,
        dataset: &[dataset::Entry],
        now: Option<SystemTime>,
    ) -> anyhow::Result<Self> {
        tokio::fs::create_dir(&root).await.unwrap();
        dataset.create_dataset(&root, now).await;

        let inner = FileSystem::new(&root)?;
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
        progress: Option<&SharedProgress>,
    ) -> impl Stream<Item = fsync::Result<fsync::Metadata>> + Send {
        self.inner.dir_entries(parent_path, progress)
    }
}

impl storage::ReadFile for Stub {
    fn read_file(
        &self,
        path: fsync::path::PathBuf,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<impl io::AsyncRead + Send>> + Send {
        self.inner.read_file(path, progress)
    }
}

impl storage::MkDir for Stub {
    fn mkdir(
        &self,
        path: &fsync::path::Path,
        parents: bool,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<()>> + Send {
        self.inner.mkdir(path, parents, progress)
    }
}

impl storage::CreateFile for Stub {
    fn create_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<fsync::Metadata>> + Send {
        self.inner.create_file(metadata, data, progress)
    }
}

impl storage::WriteFile for Stub {
    fn write_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<fsync::Metadata>> + Send {
        self.inner.write_file(metadata, data, progress)
    }
}

impl storage::Delete for Stub {
    fn delete(
        &self,
        path: &Path,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<()>> + Send {
        self.inner.delete(path, progress)
    }
}

impl fsyncd::Shutdown for Stub {
    async fn shutdown(&self) -> anyhow::Result<()> {
        let _ = fs::remove_dir_all(self.root()).await;
        Ok(())
    }
}

impl storage::Storage for Stub {}
