use std::{error, fmt, io};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Metadata {
    Directory {
        path: PathBuf,
    },
    Regular {
        path: PathBuf,
        size: u64,
        mtime: DateTime<Utc>,
    },
    Symlink {
        path: PathBuf,
        target: String,
        size: u64,
        mtime: Option<DateTime<Utc>>,
    },
    Special {
        path: PathBuf,
    },
}

impl Metadata {
    pub fn root() -> Self {
        Self::Directory {
            path: PathBuf::root(),
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Directory { path, .. } => path,
            Self::Regular { path, .. } => path,
            Self::Symlink { path, .. } => path,
            Self::Special { path, .. } => path,
        }
    }

    pub fn name(&self) -> &str {
        self.path().file_name().unwrap_or("")
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Self::Directory { .. })
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::Regular { .. })
    }

    pub fn is_symlink(&self) -> bool {
        matches!(self, Self::Symlink { .. })
    }

    pub fn is_special(&self) -> bool {
        matches!(self, Self::Special { .. })
    }

    pub fn size(&self) -> Option<u64> {
        match self {
            Self::Regular { size, .. } => Some(*size),
            Self::Symlink { size, .. } => Some(*size),
            _ => None,
        }
    }

    pub fn mtime(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Regular { mtime, .. } => Some(*mtime),
            Self::Symlink { mtime, .. } => *mtime,
            _ => None,
        }
    }

    pub fn symlink_target(&self) -> Option<&str> {
        match &self {
            Self::Symlink { target, .. } => Some(target),
            _ => None,
        }
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Metadata::root()
    }
}

pub mod tree {
    use serde::{Deserialize, Serialize};

    use crate::path::Path;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum Entry {
        Local(super::Metadata),
        Remote(super::Metadata),
        Both {
            local: super::Metadata,
            remote: super::Metadata,
        },
    }

    impl Entry {
        pub fn path(&self) -> &Path {
            match self {
                Entry::Both { local, remote } => {
                    debug_assert_eq!(local.path(), remote.path());
                    local.path()
                }
                Entry::Local(entry) => entry.path(),
                Entry::Remote(entry) => entry.path(),
            }
        }

        pub fn is_local_only(&self) -> bool {
            matches!(self, Entry::Local(..))
        }

        pub fn is_remote_only(&self) -> bool {
            matches!(self, Entry::Remote(..))
        }

        pub fn is_both(&self) -> bool {
            matches!(self, Entry::Both { .. })
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Node {
        entry: Entry,
        children: Vec<String>,
    }

    impl Node {
        pub fn new(entry: Entry, children: Vec<String>) -> Self {
            Self { entry, children }
        }

        pub fn entry(&self) -> &Entry {
            &self.entry
        }

        pub fn children(&self) -> &[String] {
            &self.children
        }

        pub fn path(&self) -> &Path {
            self.entry.path()
        }

        pub fn is_local_only(&self) -> bool {
            self.entry.is_local_only()
        }

        pub fn is_remote_only(&self) -> bool {
            self.entry.is_remote_only()
        }

        pub fn is_both(&self) -> bool {
            self.entry.is_both()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Location {
    Local,
    Remote,
    Both,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Location::Local => f.write_str("local drive"),
            Location::Remote => f.write_str("remote drive"),
            Location::Both => f.write_str("both drives"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PathError {
    NotFound(PathBuf, Option<Location>),
    Only(PathBuf, Location),
    Unexpected(PathBuf, Location),
    Illegal(PathBuf, Option<String>),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path, None) => write!(f, "No such entry: {path}"),
            Self::NotFound(path, Some(loc)) => write!(f, "Did not find '{path} on {loc}"),
            Self::Only(path, loc) => write!(f, "Could only find '{path}' on {loc}"),
            Self::Unexpected(path, loc) => write!(f, "Did not expect to find '{path}' on {loc}"),
            Self::Illegal(path, None) => write!(f, "Illegal path: {path}"),
            Self::Illegal(path, Some(reason)) => write!(f, "{reason}: {path}"),
        }
    }
}

impl error::Error for PathError {}

#[derive(Debug, Serialize, Deserialize)]
pub enum Error {
    Path(PathError),
    Io(String),
    Api(String),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(err) => err.fmt(f),
            Self::Io(msg) => write!(f, "Io error: {msg}"),
            Self::Api(msg) => write!(f, "API error: {msg}"),
            Self::Other(msg) => f.write_str(msg),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Path(err) => Some(err),
            _ => None,
        }
    }
}

impl From<PathError> for Error {
    fn from(value: PathError) -> Self {
        Self::Path(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Self::Other(value.to_string())
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

#[tarpc::service]
pub trait Fsync {
    async fn entry(path: PathBuf) -> Result<Option<tree::Node>, Error>;
    async fn copy_remote_to_local(path: PathBuf) -> Result<(), Error>;
    async fn copy_local_to_remote(path: PathBuf) -> Result<(), Error>;
}
