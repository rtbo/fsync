use std::{error, fmt, io, string::FromUtf8Error};

use camino::FromPathBufError;
use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

use crate::{
    path::{NormalizeError, PathBuf},
    Location,
};

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum PathError {
    NotFound(PathBuf, Option<Location>),
    Only(PathBuf, Location),
    Unexpected(PathBuf, Location),
    Illegal(PathBuf, Option<String>),
}

impl From<NormalizeError> for PathError {
    fn from(value: NormalizeError) -> Self {
        Self::Illegal(value.0, Some("Path can't be normalized".to_string()))
    }
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

/// An error type for RPC results
#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum Error {
    Path(PathError),
    Utf8(String),
    IllegalSymlink { path: PathBuf, target: String },
    Io(String),
    Auth(String),
    Api(String),
    Bug(String),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(err) => err.fmt(f),
            Self::Utf8(msg) => write!(f, "Non UTF-8 string: {msg}"),
            Self::IllegalSymlink { path, target } => {
                write!(f, "Illegal symlink: {path} -> {target}")
            }
            Self::Auth(msg) => write!(f, "Authorization error: {msg}"),
            Self::Io(msg) => write!(f, "IO error: {msg}"),
            Self::Api(msg) => write!(f, "API error: {msg}"),
            Self::Bug(msg) => write!(f, "Fsync bug error: {msg}"),
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

impl From<NormalizeError> for Error {
    fn from(value: NormalizeError) -> Self {
        Self::Path(value.into())
    }
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Self::Utf8(String::from_utf8_lossy(&value.into_bytes()).to_string())
    }
}

impl From<FromPathBufError> for Error {
    fn from(value: FromPathBufError) -> Self {
        Self::Utf8(value.as_path().as_os_str().to_string_lossy().to_string())
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

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn serialize_other_error() {
        let err = Error::Other("An error message".into());
        let json_err = serde_json::to_string(&err).unwrap();
        assert_eq!(json_err, r#"{"other":"An error message"}"#);
    }

    #[test]
    fn deserialize_other_error() {
        let json_err = r#"{"other":"An error message"}"#;
        let err: Error = serde_json::from_str(json_err).unwrap();
        assert_eq!(err.to_string(), "An error message");
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[macro_export]
macro_rules! io_bail {
    ($($t:tt)*) => {
        return ::core::result::Result::Err($crate::Error::Io(format!($($t)*)));
    };
}

#[macro_export]
macro_rules! api_bail {
    ($($t:tt)*) => {
        return ::core::result::Result::Err($crate::Error::Api(format!($($t)*)));
    };
}

#[macro_export]
macro_rules! auth_bail {
    ($($t:tt)*) => {
        return ::core::result::Result::Err($crate::Error::Auth(format!($($t)*)));
    };
}

#[macro_export]
macro_rules! other_bail {
    ($($t:tt)*) => {
        return ::core::result::Result::Err($crate::Error::Other(format!($($t)*)));
    };
}

#[macro_export]
macro_rules! io_error {
    ($($t:tt)*) => {
        $crate::Error::Io(format!($($t)*))
    };
}

#[macro_export]
macro_rules! auth_error {
    ($($t:tt)*) => {
        $crate::Error::Auth(format!($($t)*))
    };
}

#[macro_export]
macro_rules! api_error {
    ($($t:tt)*) => {
        $crate::Error::Api(format!($($t)*))
    };
}

#[macro_export]
macro_rules! other_error {
    ($($t:tt)*) => {
        $crate::Error::Other(format!($($t)*))
    };
}
