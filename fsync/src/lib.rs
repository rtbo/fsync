#![allow(async_fn_in_trait)]
#![feature(async_closure)]

pub mod cipher;
pub mod fs;
mod storage;

pub use crate::storage::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    #[error("file system related error")]
    Fs(#[from] crate::fs::Error),

    #[error("Google Drive error")]
    Gdrive(#[from] google_drive3::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
