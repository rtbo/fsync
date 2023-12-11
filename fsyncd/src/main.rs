mod cache;
mod config;
mod difftree;

use std::sync::Arc;

use config::PatternList;
use difftree::DiffTree;
use fsync::{fs, Result};
use tokio::time::Instant;

use crate::cache::Cache;

#[tokio::main]
async fn main() -> Result<()> {
    let ignored = Arc::new(PatternList::default());

    let local = Arc::new(fs::Storage::new("/home/remi/drive"));
    let local = Cache::new_from_storage(local, ignored.clone());

    let auth = gdrive::Storage::auth(None).await;
    let remote = Arc::new(gdrive::Storage::new(auth));
    let remote = Cache::new_from_storage(remote, ignored);

    let start = Instant::now();
    let (local, remote) = tokio::join!(local, remote);
    let elapsed_cache = start.elapsed();

    let local = local?;
    let remote = remote?;

    let start = Instant::now();
    let tree = DiffTree::from_cache(&local, &remote);
    let elapsed_diff = start.elapsed();

    tree.print_out();

    println!("caching took {elapsed_cache:?}");
    println!("diffing took {elapsed_diff:?}");

    Ok(())
}
