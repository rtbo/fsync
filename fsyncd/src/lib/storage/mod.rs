use fsync::{path::PathBuf, Metadata};
use futures::{Future, Stream};
use tokio::io;

pub mod cache;
pub mod fs;
pub mod gdrive;

pub trait DirEntries {
    fn dir_entries(
        &self,
        parent_path: Option<PathBuf>,
    ) -> impl Stream<Item = anyhow::Result<Metadata>> + Send;
}

pub trait ReadFile {
    fn read_file(
        &self,
        path: PathBuf,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead + Send>> + Send;
}

pub trait CreateFile {
    fn create_file(
        &self,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<Metadata>> + Send;
}

pub trait Storage: Clone + DirEntries + ReadFile + CreateFile + Send + Sync + 'static {}

pub mod id {
    use fsync::{Metadata, PathIdBuf};
    use futures::{Future, Stream};
    use tokio::io;

    pub trait DirEntries {
        fn dir_entries(
            &self,
            parent_path_id: Option<PathIdBuf>,
        ) -> impl Stream<Item = anyhow::Result<(String, Metadata)>> + Send;
    }

    pub trait ReadFile {
        fn read_file(
            &self,
            path_id: PathIdBuf,
        ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead + Send>> + Send;
    }

    pub trait CreateFile {
        fn create_file(
            &self,
            metadata: &Metadata,
            data: impl io::AsyncRead + Send,
        ) -> impl Future<Output = anyhow::Result<(String, Metadata)>> + Send;
    }

    pub trait Storage: Clone + DirEntries + ReadFile + CreateFile + Send + Sync + 'static {}
}
