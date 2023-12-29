#![allow(async_fn_in_trait)]
#![feature(async_closure)]

use anyhow::Context;
use camino::Utf8Path;
use serde::{Deserialize, Serialize};

pub mod cipher;
pub mod config;
pub mod http;
pub mod ipc;
pub mod loc;
pub mod oauth2;
pub mod provider;
pub mod tree;

mod storage;
pub use crate::storage::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub local_dir: String,
    pub provider: provider::Provider,
}

impl Config {
    pub async fn load_from_file(path: &Utf8Path) -> anyhow::Result<Self> {
        let config_json = tokio::fs::read(&path)
            .await
            .with_context(|| format!("Failed to read config from {path}"))?;
        let config_json = std::str::from_utf8(&config_json)?;
        Ok(serde_json::from_str(config_json)?)
    }
}
