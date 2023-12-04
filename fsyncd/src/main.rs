mod cache;
mod config;
mod tree;

use std::sync::Arc;

use fsync::fs;
use fsync::Result;

use crate::cache::Cache;
use crate::config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::new();

    let st = Arc::new(fs::Storage::new("/home/remi/Documents"));
    let cache = Cache::new();
    cache.populate(st).await?;
    cache.print_tree();
    Ok(())
}
