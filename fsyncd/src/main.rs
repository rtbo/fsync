use std::sync::Arc;

use fsync::cache::Cache;
use fsync::fs;
use fsync::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let st = Arc::new(fs::Storage::new("/home/remi/Documents"));
    let cache = Cache::new();
    cache.populate(st).await?;
    cache.print_tree();
    Ok(())
}
