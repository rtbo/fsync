#![allow(async_fn_in_trait)]
#![feature(async_closure)]


pub mod cipher;
pub mod storage;
pub mod fs;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("file system related error")]
    Fs(#[from] crate::fs::Error),
    #[error("I/O error")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
