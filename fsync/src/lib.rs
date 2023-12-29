#![allow(async_fn_in_trait)]
#![feature(async_closure)]

use camino::Utf8Path;
use serde::{Deserialize, Serialize};

pub mod cipher;
pub mod config;
pub mod http;
pub mod ipc;
pub mod loc;
pub mod provider;
pub mod oauth2;
pub mod tree;

mod storage;
pub use crate::storage::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub local_dir: String,
    pub provider: provider::Provider,
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
    Http(#[from] http::Error),

    #[error("Custom error: {0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, Error>;
