#![allow(async_fn_in_trait)]

use std::{cmp, fmt, str, time};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod cipher;
pub mod config;
pub mod loc;
pub mod oauth2;

mod error;
mod fsync;

pub use crate::{
    config::{Config, ProviderConfig},
    error::*,
    fsync::*,
};

pub mod path;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::GoogleDrive => f.write_str("Google Drive"),
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
