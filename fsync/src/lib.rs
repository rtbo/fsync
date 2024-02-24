#![allow(async_fn_in_trait)]

use std::{cmp, fmt, str, time};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod cipher;
pub mod config;
pub mod loc;
pub mod oauth2;

mod conflict;
mod error;
mod fsync;

pub use crate::{
    config::{Config, ProviderConfig},
    conflict::Conflict,
    error::*,
    fsync::*,
};

pub mod path;
pub mod stat;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StorageLoc {
    Local,
    Remote,
}

impl StorageLoc {
    pub fn opposite(self) -> Self {
        match self {
            StorageLoc::Local => StorageLoc::Remote,
            StorageLoc::Remote => StorageLoc::Local,
        } 
    }
}

impl fmt::Display for StorageLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageLoc::Local => f.write_str("local drive"),
            StorageLoc::Remote => f.write_str("remote drive"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StorageDir {
    LocalToRemote,
    RemoteToLocal,
}

impl StorageDir {
    pub fn opposite(self) -> Self {
        match self {
            StorageDir::LocalToRemote => StorageDir::RemoteToLocal,
            StorageDir::RemoteToLocal => StorageDir::LocalToRemote,
        } 
    }

    pub fn src(self) -> StorageLoc {
        match self {
            StorageDir::LocalToRemote => StorageLoc::Local,
            StorageDir::RemoteToLocal => StorageLoc::Remote,
        }
    }

    pub fn dest(self) -> StorageLoc {
        match self {
            StorageDir::LocalToRemote => StorageLoc::Remote,
            StorageDir::RemoteToLocal => StorageLoc::Local,
        }
    }
}

impl fmt::Display for StorageDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageDir::LocalToRemote => f.write_str("local to remote drive"),
            StorageDir::RemoteToLocal => f.write_str("remote to local drive"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Location {
    Local,
    Remote,
    Both,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Location::Local => f.write_str("local drive"),
            Location::Remote => f.write_str("remote drive"),
            Location::Both => f.write_str("both drives"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Provider {
    GoogleDrive,
    LocalFs,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::GoogleDrive => f.write_str("Google Drive"),
            Provider::LocalFs => f.write_str("Local FileSystem"),
        }
    }
}

pub const MTIME_TOL: time::Duration = time::Duration::from_secs(1);

pub fn compare_mtime(lhs: DateTime<Utc>, rhs: DateTime<Utc>) -> cmp::Ordering {
    if lhs + MTIME_TOL < rhs {
        cmp::Ordering::Less
    } else if lhs - MTIME_TOL > rhs {
        cmp::Ordering::Greater
    } else {
        cmp::Ordering::Equal
    }
}

pub fn compare_mtime_opt(
    lhs: Option<DateTime<Utc>>,
    rhs: Option<DateTime<Utc>>,
) -> Option<cmp::Ordering> {
    if let (Some(lhs), Some(rhs)) = (lhs, rhs) {
        Some(compare_mtime(lhs, rhs))
    } else {
        None
    }
}
