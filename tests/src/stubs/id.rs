use fsync::path::{FsPath, PathBuf};
use fsyncd::{storage::{
    fs::FileSystem,
    id::{self, IdBuf},
    DirEntries, ReadFile, MkDir, CreateFile,
}, Shutdown};
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
        let dst = utils::temp_path(Some("id"), None);
        println!("copying {src} to {dst}");
        utils::copy_dir_all(src, &dst).await?;
        let inner = FileSystem::new(&dst)?;
        Ok(Self { inner })
    }
}

impl id::DirEntries for Stub {
    fn dir_entries(
        &self,
        _parent_id: Option<id::IdBuf>,
        parent_path: fsync::path::PathBuf,
    ) -> impl Stream<Item = anyhow::Result<(IdBuf, fsync::Metadata)>> + Send {
        self.inner
            .dir_entries(parent_path)
            .map_ok(|md| (IdBuf::from(md.path().as_str()), md))
    }
}

impl id::ReadFile for Stub {
    async fn read_file(&self, id: IdBuf) -> anyhow::Result<impl io::AsyncRead + Send> {
        let path = PathBuf::from(id.into_string());
        self.inner.read_file(path).await
    }
}

impl id::MkDir for Stub {
    async fn mkdir(&self, parent_id: Option<&id::Id>, name: &str) -> anyhow::Result<IdBuf> {
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
        ) -> anyhow::Result<(IdBuf, fsync::Metadata)> {
        let metadata = self.inner.create_file(metadata, data).await?;
        let id = IdBuf::from(metadata.path());
        Ok((id, metadata))
    }
}

impl Shutdown for Stub {}

impl id::Storage for Stub {}
