use std::{borrow::Borrow, fmt, ops::Deref};

use fsync::{path::Path, Metadata};
use futures::{Future, Stream};
use serde::{Deserialize, Serialize};
use tokio::io;

use crate::{SharedProgress, Shutdown};

pub trait DirEntries {
    fn dir_entries(
        &self,
        parent_id: Option<&Id>,
        parent_path: &Path,
        progress: Option<&SharedProgress>,
    ) -> impl Stream<Item = fsync::Result<(IdBuf, Metadata)>> + Send;
}

pub trait ReadFile {
    fn read_file(
        &self,
        id: IdBuf,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<impl io::AsyncRead + Send>> + Send;
}

pub trait MkDir {
    fn mkdir(
        &self,
        parent_id: Option<&Id>,
        name: &str,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<IdBuf>> + Send;
}

pub trait CreateFile {
    fn create_file(
        &self,
        parent_id: Option<&Id>,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<(IdBuf, Metadata)>> + Send;
}

pub trait WriteFile {
    fn write_file(
        &self,
        id: &Id,
        parent_id: Option<&Id>,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<Metadata>> + Send;
}

pub trait CopyFile {
    fn copy_file(
        &self,
        src_id: &Id,
        dest_parent_id: Option<&Id>,
        dest_path: &Path,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<(IdBuf, Metadata)>> + Send;
}

/// A trait to delete files or folders
pub trait Delete {
    /// Deletes the file or folder referred to by `id`.
    /// If `id` refers to a non-empty folder, all the folder content is also deleted.
    fn delete(
        &self,
        id: &Id,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<()>> + Send;
}

/// A trait for an ID-based storage
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

#[repr(transparent)]
pub struct Id {
    inner: str,
}

impl Id {
    pub fn new<S: AsRef<str> + ?Sized>(id: &S) -> &Id {
        unsafe { &*(id.as_ref() as *const str as *const Id) }
    }

    pub fn as_str(&self) -> &str {
        &self.inner
    }

    pub fn to_id_buf(&self) -> IdBuf {
        IdBuf {
            inner: self.inner.to_string(),
        }
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Id(")?;
        fmt::Debug::fmt(&self.inner, f)?;
        f.write_str(")")
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl Default for &Id {
    fn default() -> Self {
        Id::new("")
    }
}

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct IdBuf {
    inner: String,
}

impl IdBuf {
    pub fn new() -> Self {
        Self {
            inner: String::new(),
        }
    }

    pub fn as_id(&self) -> &Id {
        Id::new(self.inner.as_str())
    }

    pub fn into_string(self) -> String {
        self.inner
    }
}

impl From<String> for IdBuf {
    fn from(value: String) -> Self {
        IdBuf { inner: value }
    }
}

impl<T: ?Sized + AsRef<str>> From<&T> for IdBuf {
    /// Converts a borrowed [`str`] to a [`IdBuf`].
    ///
    /// Allocates a [`IdBuf`] and copies the data into it.
    #[inline]
    fn from(s: &T) -> IdBuf {
        IdBuf::from(s.as_ref().to_string())
    }
}

impl fmt::Debug for IdBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IdBuf(")?;
        fmt::Debug::fmt(&self.inner, f)?;
        f.write_str(")")
    }
}

impl fmt::Display for IdBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl Deref for IdBuf {
    type Target = Id;

    fn deref(&self) -> &Id {
        self.as_id()
    }
}

impl Borrow<Id> for IdBuf {
    fn borrow(&self) -> &Id {
        self.as_id()
    }
}

impl ToOwned for Id {
    type Owned = IdBuf;

    fn to_owned(&self) -> IdBuf {
        self.to_id_buf()
    }
}
