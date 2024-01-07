use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;

use fsync::{path::PathBuf, Metadata};
use futures::{future, Future, Stream};
use serde::{Deserialize, Serialize};
use tokio::io;

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

pub trait DirEntries {
    fn dir_entries(
        &self,
        parent_id: Option<IdBuf>,
        parent_path: PathBuf,
    ) -> impl Stream<Item = anyhow::Result<(IdBuf, Metadata)>> + Send;
}

pub trait ReadFile {
    fn read_file(
        &self,
        id: IdBuf,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead + Send>> + Send;
}

pub trait CreateFile {
    fn create_file(
        &self,
        parent_id: Option<&Id>,
        metadata: &Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<(IdBuf, Metadata)>> + Send;
}

pub trait Storage: Clone + DirEntries + ReadFile + CreateFile + Send + Sync + 'static {
    fn shutdown(&self) -> impl Future<Output = ()> + Send {
        future::ready(())
    }
}
