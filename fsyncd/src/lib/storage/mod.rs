use fsync::{
    path::{Path, PathBuf},
    Metadata,
};
use futures::{Future, Stream};
use tokio::io;

use crate::{SharedOpState, Shutdown};

pub mod cache;
pub mod drive;
pub mod fs;
pub mod id;

pub trait DirEntries {
    fn dir_entries(
        &self,
        parent_path: &Path,
        op_state: Option<&SharedOpState>,
    ) -> impl Stream<Item = fsync::Result<Metadata>> + Send;
}

pub trait ReadFile {
    fn read_file(
        &self,
        path: PathBuf,
        op_state: Option<&SharedOpState>,
    ) -> impl Future<Output = fsync::Result<impl io::AsyncRead + Send>> + Send;
}

pub trait MkDir {
    fn mkdir(
        &self,
        path: &Path,
        parents: bool,
        op_state: Option<&SharedOpState>,
    ) -> impl Future<Output = fsync::Result<()>> + Send;
}

pub trait CreateFile {
    fn create_file(
        &self,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
        op_state: Option<&SharedOpState>,
    ) -> impl Future<Output = fsync::Result<Metadata>> + Send;
}

pub trait WriteFile {
    fn write_file(
        &self,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
        op_state: Option<&SharedOpState>,
    ) -> impl Future<Output = fsync::Result<Metadata>> + Send;
}

/// A trait to delete files or folders
pub trait Delete {
    /// Deletes the file or folder pointed to by `path`.
    /// Only empty folders can be deleted.
    fn delete(
        &self,
        path: &Path,
        op_state: Option<&SharedOpState>,
    ) -> impl Future<Output = fsync::Result<()>> + Send;
}

/// A trait for path-based storage
pub trait Storage:
    Clone
    + DirEntries
    + ReadFile
    + MkDir
    + CreateFile
    + WriteFile
    + Delete
    + Shutdown
    + Send
    + Sync
    + 'static
{
}
