mod cache;
mod config;
mod tree;

use std::sync::Arc;

use fsync::Result;

use crate::cache::Cache;

#[tokio::main]
async fn main() -> Result<()> {
    let auth = gdrive::Storage::auth(None).await;
    let st1 = Arc::new(gdrive::Storage::new(auth));
    let st = st1;
    let cache = Cache::new_from_storage(st).await?;
    cache.print_tree();
    Ok(())
}
