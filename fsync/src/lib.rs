#![allow(async_fn_in_trait)]
#![feature(async_closure)]

use camino::{Utf8Path, Utf8PathBuf};
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

impl Config {
    pub async fn load_from_file(path: &Utf8Path) -> Result<Self> {
        let config_json = match tokio::fs::read(&path).await {
            Ok(data) => data,
            Err(err) => {
                return Err(Error::Io(std::io::Error::new(
                    err.kind(),
                    format!("Could not open config file {path}: {err}"),
                )));
            }
        };
        let config_json = std::str::from_utf8(&config_json)?;
        Ok(serde_json::from_str(config_json)?)
    }
}

pub fn home_dir() -> Result<Utf8PathBuf> {
    let dir = dirs::home_dir().ok_or_else(|| Error::Custom("Can't get HOME directory".into()))?;
    Ok(Utf8PathBuf::from_path_buf(dir).unwrap())
}

pub fn cache_dir() -> Result<Utf8PathBuf> {
    let dir = dirs::cache_dir().ok_or_else(|| Error::Custom("Can't get cache directory".into()))?;
    let dir = Utf8PathBuf::from_path_buf(dir).expect("Non Utf8 path");
    Ok(dir.join("fsync"))
}

pub fn config_dir() -> Result<Utf8PathBuf> {
    let dir =
        dirs::config_dir().ok_or_else(|| Error::Custom("Can't get config directory".into()))?;
    let dir = Utf8PathBuf::from_path_buf(dir).expect("Non Utf8 path");
    Ok(dir.join("fsync"))
}

pub fn instance_cache_dir(name: &str) -> Result<Utf8PathBuf> {
    cache_dir().map(|d| d.join(name))
}

pub fn instance_config_dir(name: &str) -> Result<Utf8PathBuf> {
    config_dir().map(|d| d.join(name))
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
