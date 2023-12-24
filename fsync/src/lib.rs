#![allow(async_fn_in_trait)]
#![feature(async_closure)]

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

pub mod backend;
pub mod cache;
pub mod cipher;
pub mod config;
pub mod difftree;
pub mod ipc;
pub mod oauth2;
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
    let dir = dirs::home_dir().ok_or_else(|| Error::Custom("Can't get HOME directory".into()))?;
    Ok(Utf8PathBuf::from_path_buf(dir).unwrap())
}

pub fn get_cache_dir() -> Result<Utf8PathBuf> {
    let dir = dirs::cache_dir().ok_or_else(|| Error::Custom("Can't get cache directory".into()))?;
    let dir = Utf8PathBuf::from_path_buf(dir).expect("Non Utf8 path");
    Ok(dir.join("fsync"))
}

pub fn get_config_dir() -> Result<Utf8PathBuf> {
    let dir =
        dirs::config_dir().ok_or_else(|| Error::Custom("Can't get config directory".into()))?;
    let dir = Utf8PathBuf::from_path_buf(dir).expect("Non Utf8 path");
    Ok(dir.join("fsync"))
}

pub fn get_instance_cache_dir(name: &str) -> Result<Utf8PathBuf> {
    get_cache_dir().map(|d| d.join(name))
}

pub fn get_instance_config_dir(name: &str) -> Result<Utf8PathBuf> {
    get_config_dir().map(|d| d.join(name))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error")]
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

    #[error("file system related error")]
    Fs(#[from] crate::backend::fs::Error),

    #[error("Google Drive error")]
    GoogleDrive(#[from] google_drive3::Error),

    #[error("Custom error")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, Error>;
