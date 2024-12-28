#![cfg(test)]

use std::sync::{Arc, Once};

use dataset::Dataset;
use fsyncd::{service::Service, storage::cache::CacheStorage};

//mod config;
mod dataset;
mod harness;
mod utils;
mod stubs {
    //pub mod drive;
    pub mod fs;
    pub mod id;
}
mod tests;

use harness::Harness;
use stubs::{fs, id};

type CacheHarness = Harness<fs::Stub, CacheStorage<id::Stub>>;

static LOG_INIT: Once = Once::new();

async fn harness<D: Into<Dataset>>(dataset: D) -> CacheHarness {
    LOG_INIT.call_once(env_logger::init);

    let dataset = dataset.into();

    let root = utils::temp_path(Some("fsync-fs"), None);
    tokio::fs::create_dir(&root).await.unwrap();

    let (local, remote) = dataset.create_fs(&root).await;

    let service = Arc::new(Service::new(local, remote, root).await.unwrap());

    Harness { service }
}
