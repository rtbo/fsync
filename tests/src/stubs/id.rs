use std::time::SystemTime;

use fsync::path::{FsPath, Path, PathBuf};
use fsyncd::{
    storage::{
        fs::FileSystem,
        id::{self, IdBuf},
        CopyFile, CreateFile, Delete, DirEntries, MkDir, ReadFile, WriteFile,
    },
    SharedProgress, Shutdown,
};
use futures::prelude::*;
use tokio::io;

use crate::dataset::{self, CreateFs};

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
        entries: &[dataset::Entry],
        now: Option<SystemTime>,
    ) -> anyhow::Result<Self> {
        tokio::fs::create_dir(&root).await.unwrap();
        entries.create_fs(&root, now).await;

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
        progress: Option<&SharedProgress>,
    ) -> impl Stream<Item = fsync::Result<(IdBuf, fsync::Metadata)>> + Send {
        self.inner
            .dir_entries(parent_path, progress)
            .map_ok(|md| (IdBuf::from(md.path().as_str()), md))
    }
}

impl id::ReadFile for Stub {
    async fn read_file(
        &self,
        id: IdBuf,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<impl io::AsyncRead + Send> {
        let path = PathBuf::from(id.into_string());
        self.inner.read_file(path, progress).await
    }
}

impl id::MkDir for Stub {
    async fn mkdir(
        &self,
        parent_id: Option<&id::Id>,
        name: &str,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<IdBuf> {
        let parent_path = parent_id.map(PathBuf::from).unwrap_or_else(PathBuf::root);
        let path = parent_path.join(name);
        self.inner.mkdir(&path, false, progress).await?;
        Ok(IdBuf::from(path.into_string()))
    }
}

impl id::CreateFile for Stub {
    async fn create_file(
        &self,
        _parent_id: Option<&id::Id>,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<(IdBuf, fsync::Metadata)> {
        let metadata = self.inner.create_file(metadata, data, progress).await?;
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
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<fsync::Metadata> {
        let metadata = self.inner.write_file(metadata, data, progress).await?;
        Ok(metadata)
    }
}

impl id::CopyFile for Stub {
    async fn copy_file(
        &self,
        src_id: &id::Id,
        _dest_parent_id: Option<&id::Id>,
        dest_path: &Path,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<(IdBuf, fsync::Metadata)> {
        let src = PathBuf::from(src_id.as_str());
        let metadata = self.inner.copy_file(&src, dest_path, progress).await?;
        let id = IdBuf::from(dest_path.as_str());
        Ok((id, metadata))
    }
}

impl id::Delete for Stub {
    async fn delete(&self, id: &id::Id, progress: Option<&SharedProgress>) -> fsync::Result<()> {
        let path = PathBuf::from(id.as_str());
        self.inner.delete(&path, progress).await
    }
}

impl Shutdown for Stub {}

impl id::Storage for Stub {}
