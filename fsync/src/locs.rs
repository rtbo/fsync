//! Locations module

use std::fmt;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigDir(Utf8PathBuf);

impl fmt::Display for ConfigDir {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl ConfigDir {
    pub fn new(instance_name: &str) -> Result<ConfigDir> {
        user_config_dir().map(|d| ConfigDir(d.join(instance_name)))
    }

    pub fn path(&self) -> &Utf8Path {
        &self.0
    }

    pub fn exists(&self) -> bool {
        self.0.exists()
    }

    pub fn join<P: AsRef<Utf8Path>>(&self, path: P) -> Utf8PathBuf {
        self.0.join(path)
    }

    pub fn config_path(&self) -> Utf8PathBuf {
        self.join("config.json")
    }

    pub fn client_secret_path(&self) -> Utf8PathBuf {
        self.join("client_secret.json")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheDir(Utf8PathBuf);

impl fmt::Display for CacheDir {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl CacheDir {
    pub fn new(instance_name: &str) -> Result<CacheDir> {
        user_cache_dir().map(|d| CacheDir(d.join(instance_name)))
    }

    pub fn path(&self) -> &Utf8Path {
        &self.0
    }

    pub fn exists(&self) -> bool {
        self.0.exists()
    }

    pub fn join<P: AsRef<Utf8Path>>(&self, path: P) -> Utf8PathBuf {
        self.0.join(path)
    }

    pub fn token_cache_path(&self) -> Utf8PathBuf {
        self.join("token_cache.json")
    }
}

pub fn user_home_dir() -> Result<Utf8PathBuf> {
    let dir = dirs::home_dir().ok_or_else(|| Error::Custom("Can't get HOME directory".into()))?;
    Ok(Utf8PathBuf::from_path_buf(dir).unwrap())
}

fn user_cache_dir() -> Result<Utf8PathBuf> {
    let dir = dirs::cache_dir().ok_or_else(|| Error::Custom("Can't get cache directory".into()))?;
    let dir = Utf8PathBuf::from_path_buf(dir).expect("Non Utf8 path");
    Ok(dir.join("fsync"))
}

pub fn user_config_dir() -> Result<Utf8PathBuf> {
    let dir =
        dirs::config_dir().ok_or_else(|| Error::Custom("Can't get config directory".into()))?;
    let dir = Utf8PathBuf::from_path_buf(dir).expect("Non Utf8 path");
    Ok(dir.join("fsync"))
}
