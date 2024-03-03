use std::time::SystemTime;

use fsync::path::{FsPath, Path, PathBuf};
use fsyncd::{
    storage::{
        fs::FileSystem,
        id::{self, IdBuf},
        CreateFile, Delete, DirEntries, MkDir, ReadFile, WriteFile,
    }, SharedOpState, Shutdown
};
use futures::prelude::*;
use tokio::io;

use crate::dataset::{self, CreateDataset};

/// Stub that fakes an Id based Storage with filesystem
/// Ids are paths that are:
///  - normalized
///  - absolute from storage root
#[derive(Clone)]
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
}

impl Drop for Stub {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.inner.root()).unwrap();
    }
}

impl id::DirEntries for Stub {
    fn dir_entries(
        &self,
        _parent_id: Option<&id::Id>,
        parent_path: &Path,
        op_state: Option<&SharedOpState>,
    ) -> impl Stream<Item = fsync::Result<(IdBuf, fsync::Metadata)>> + Send {
        self.inner
            .dir_entries(parent_path, op_state)
            .map_ok(|md| (IdBuf::from(md.path().as_str()), md))
    }
}

impl id::ReadFile for Stub {
    async fn read_file(&self, id: IdBuf, op_state: Option<&SharedOpState>) -> fsync::Result<impl io::AsyncRead + Send> {
        let path = PathBuf::from(id.into_string());
        self.inner.read_file(path, op_state).await
    }
}

impl id::MkDir for Stub {
    async fn mkdir(&self, parent_id: Option<&id::Id>, name: &str, op_state: Option<&SharedOpState>) -> fsync::Result<IdBuf> {
        let parent_path = parent_id.map(PathBuf::from).unwrap_or_else(PathBuf::root);
        let path = parent_path.join(name);
        self.inner.mkdir(&path, false, op_state).await?;
        Ok(IdBuf::from(path.into_string()))
    }
}

impl id::CreateFile for Stub {
    async fn create_file(
        &self,
        _parent_id: Option<&id::Id>,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
        op_state: Option<&SharedOpState>,
    ) -> fsync::Result<(IdBuf, fsync::Metadata)> {
        let metadata = self.inner.create_file(metadata, data, op_state).await?;
        let id: String = metadata.path().normalize()?.into_string();
        Ok((IdBuf::from(id), metadata))
    }
}

impl id::WriteFile for Stub {
    async fn write_file(
        &self,
        _id: &id::Id,
        _parent_id: Option<&id::Id>,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
        op_state: Option<&SharedOpState>,
    ) -> fsync::Result<fsync::Metadata> {
        let metadata = self.inner.write_file(metadata, data, op_state).await?;
        Ok(metadata)
    }
}

impl id::Delete for Stub {
    async fn delete(&self, id: &id::Id, op_state: Option<&SharedOpState>) -> fsync::Result<()> {
        let path = PathBuf::from(id.as_str());
        self.inner.delete(&path, op_state).await
    }
}

impl Shutdown for Stub {}

impl id::Storage for Stub {}
