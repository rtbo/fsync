use fsync::{Metadata, PathId};
use futures::{Future, Stream};
use tokio::io;

pub mod cache;
pub mod fs;
pub mod gdrive;

pub trait DirEntries {
    fn dir_entries(
        &self,
        parent_path_id: Option<PathId>,
    ) -> impl Stream<Item = anyhow::Result<Metadata>> + Send;
}

pub trait ReadFile {
    fn read_file<'a>(
        &'a self,
        path_id: PathId<'a>,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead + Send>> + Send + 'a;
}

pub trait CreateFile {
    fn create_file(
        &self,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<Metadata>> + Send;
}

pub trait Storage: Clone + DirEntries + ReadFile + CreateFile + Send + Sync + 'static {}
