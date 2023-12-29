pub mod service;
pub mod storage;
pub mod tree;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Utf-8 error")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("Var Error")]
    Var(#[from] std::env::VarError),

    #[error("Serde JSON")]
    SerdeJson(#[from] serde_json::Error),

    #[error("Bincode")]
    Bincode(#[from] bincode::Error),

    #[error("OAuth2")]
    OAuth2(#[from] yup_oauth2::Error),

    #[error("Hyper")]
    Hyper(#[from] hyper::Error),

    #[error("{0}")]
    Http(#[from] fsync::http::Error),

    #[error("file system related error: {0}")]
    Fs(#[from] crate::storage::fs::Error),

    #[error("Custom error: {0}")]
    Custom(String),
}

impl From<fsync::Error> for Error {
    fn from(error: fsync::Error) -> Self {
        match error {
            fsync::Error::Io(err) => Error::Io(err),
            fsync::Error::Utf8(err) => Error::Utf8(err),
            fsync::Error::Var(err) => Error::Var(err),
            fsync::Error::SerdeJson(err) => Error::SerdeJson(err),
            fsync::Error::Bincode(err) => Error::Bincode(err),
            fsync::Error::OAuth2(err) => Error::OAuth2(err),
            fsync::Error::Hyper(err) => Error::Hyper(err),
            fsync::Error::Http(err) => Error::Http(err),
            fsync::Error::Custom(err) => Error::Custom(err),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
