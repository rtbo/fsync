use fsync::path::{FsPath, Path, PathBuf};
use fsyncd::{
    storage::{
        fs::FileSystem,
        id::{self, IdBuf},
        CreateFile, Delete, DirEntries, MkDir, ReadFile, WriteFile,
    },
    Shutdown,
};
use futures::prelude::*;
use tokio::io;

use crate::utils;

/// Stub that fakes an Id based Storage with filesystem
/// Ids are paths that are:
///  - normalized
///  - absolute from storage root
#[derive(Clone)]
pub struct Stub {
    inner: FileSystem,
}

impl Stub {
    pub async fn new(src: &FsPath) -> anyhow::Result<Self> {
        let dst = utils::temp_path(Some("fsync-id"), None);
        utils::copy_dir_all(src, &dst).await?;
        let inner = FileSystem::new(&dst)?;
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
    ) -> impl Stream<Item = fsync::Result<(IdBuf, fsync::Metadata)>> + Send {
        self.inner
            .dir_entries(parent_path)
            .map_ok(|md| (IdBuf::from(md.path().as_str()), md))
    }
}

impl id::ReadFile for Stub {
    async fn read_file(&self, id: IdBuf) -> fsync::Result<impl io::AsyncRead + Send> {
        let path = PathBuf::from(id.into_string());
        self.inner.read_file(path).await
    }
}

impl id::MkDir for Stub {
    async fn mkdir(&self, parent_id: Option<&id::Id>, name: &str) -> fsync::Result<IdBuf> {
        let parent_path = parent_id.map(PathBuf::from).unwrap_or_else(PathBuf::root);
        let path = parent_path.join(name);
        self.inner.mkdir(&path, false).await?;
        Ok(IdBuf::from(path.into_string()))
    }
}

impl id::CreateFile for Stub {
    async fn create_file(
        &self,
        _parent_id: Option<&id::Id>,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> fsync::Result<(IdBuf, fsync::Metadata)> {
        let metadata = self.inner.create_file(metadata, data).await?;
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
    ) -> fsync::Result<fsync::Metadata> {
        let metadata = self.inner.write_file(metadata, data).await?;
        Ok(metadata)
    }
}

impl id::Delete for Stub {
    async fn delete(&self, id: &id::Id) -> fsync::Result<()> {
        let path = PathBuf::from(id.as_str());
        self.inner.delete(&path).await
    }
}

impl Shutdown for Stub {}

impl id::Storage for Stub {}
