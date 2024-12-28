use fsync::{
    path::{Path, PathBuf},
    Metadata,
};
use futures::{Future, Stream};
use tokio::io;

use crate::{SharedProgress, Shutdown};

pub mod cache;
pub mod drive;
pub mod fs;
pub mod id;

pub trait Exists {
    fn exists(&self, path: &Path) -> impl Future<Output = fsync::Result<bool>> + Send;
}

pub trait DirEntries {
    fn dir_entries(
        &self,
        parent_path: &Path,
        progress: Option<&SharedProgress>,
    ) -> impl Stream<Item = fsync::Result<Metadata>> + Send;
}

pub trait ReadFile {
    fn read_file(
        &self,
        path: PathBuf,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<impl io::AsyncRead + Send>> + Send;
}

pub trait MkDir {
    fn mkdir(
        &self,
        path: &Path,
        parents: bool,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<()>> + Send;
}

pub trait CreateFile {
    fn create_file(
        &self,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<Metadata>> + Send;
}

pub trait WriteFile {
    fn write_file(
        &self,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<Metadata>> + Send;
}

/// A trait to copy files within the storage
pub trait CopyFile {
    /// Copies the file from `src` to `dest`.
    fn copy_file(
        &self,
        src: &Path,
        dest: &Path,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<Metadata>> + Send;
}

/// A trait to move or rename files or directories within the storage
pub trait MoveEntry {
    /// Moves the file or directory from `src` to `dest`.
    fn move_entry(
        &self,
        src: &Path,
        dest: &Path,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<Metadata>> + Send;
}

/// A trait to delete files or folders
pub trait Delete {
    /// Deletes the file or folder pointed to by `path`.
    /// Only empty folders can be deleted.
    fn delete(
        &self,
        path: &Path,
        progress: Option<&SharedProgress>,
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
    + CopyFile
    + Delete
    + Shutdown
    + Send
    + Sync
    + 'static
{
}

/// A trait for local storage
pub trait LocalStorage : Storage + Exists + MoveEntry {}
