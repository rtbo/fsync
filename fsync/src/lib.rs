#![allow(async_fn_in_trait)]
#![feature(async_closure)]

pub mod cipher;
pub mod storage;
pub mod fs;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
