#![allow(async_fn_in_trait)]
#![feature(async_closure)]

use std::{fmt, str};

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
