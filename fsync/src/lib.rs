#![allow(async_fn_in_trait)]
#![feature(async_closure)]

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

pub mod cipher;
pub mod fs;
mod storage;

pub use crate::storage::*;

#[derive(Debug, Serialize, Deserialize)]
pub enum Provider {
    GoogleDrive,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub local_dir: String,
    pub provider: Provider,
}

pub fn get_home() -> Result<Utf8PathBuf> {
    std::env::var("HOME")
        .map(|v| v.into())
        .map_err(|err| err.into())
}

pub fn get_config_dir() -> Result<Utf8PathBuf> {
    get_home().map(|h| [h.as_str(), ".config", "fsync"].iter().collect())
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    #[error("file system related error")]
    Fs(#[from] crate::fs::Error),

    #[error("Utf-8 error")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("Var Error")]
    Var(#[from] std::env::VarError),

    #[error("Serde JSON")]
    SerdeJson(#[from] serde_json::Error),

    #[error("OAuth2")]
    OAuth2(#[from] yup_oauth2::Error),

    #[error("Google Drive error")]
    GoogleDrive(#[from] google_drive3::Error),

    #[error("Custom error")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, Error>;
