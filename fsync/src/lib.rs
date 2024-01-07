#![allow(async_fn_in_trait)]
#![feature(async_closure)]

use std::{fmt, str, time, cmp};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod cipher;
pub mod config;
pub mod loc;
pub mod oauth;

mod fsync;

pub use crate::config::Config;
pub use crate::fsync::*;

pub mod path;

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

pub fn compare_mtime_opt(lhs: Option<DateTime<Utc>>, rhs: Option<DateTime<Utc>>) -> Option<cmp::Ordering> {
    if let (Some(lhs), Some(rhs)) = (lhs, rhs) {
        Some(compare_mtime(lhs, rhs))
    } else {
        None
    }
}
