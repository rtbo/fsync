use fsync::{
    path::{Path, PathBuf},
    Metadata,
};
use futures::{Future, Stream};
use tokio::io;

use crate::PersistCache;

pub mod cache;
pub mod fs;
pub mod gdrive;
pub mod id;

pub trait DirEntries {
    fn dir_entries(
        &self,
        parent_path: PathBuf,
    ) -> impl Stream<Item = anyhow::Result<Metadata>> + Send;
}

pub trait ReadFile {
    fn read_file(
        &self,
        path: PathBuf,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead + Send>> + Send;
}

pub trait MkDir {
    fn mkdir(&self, path: &Path, parents: bool) -> impl Future<Output = anyhow::Result<()>> + Send;
}

pub trait CreateFile {
    fn create_file(
        &self,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<Metadata>> + Send;
}

pub trait Storage:
    Clone + DirEntries + ReadFile + MkDir + CreateFile + PersistCache + Send + Sync + 'static
{
}
