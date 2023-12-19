use std::sync::Arc;

use fsync::cache::Cache;
use fsync::config::PatternList;
use fsync::difftree::DiffTree;
use fsync::{oauth2, Config, Error, Provider, Result};
use service::Service;

mod service;

#[tokio::main]
async fn main() -> Result<()> {
    let instance_name = std::env::var("FSYNCD_INSTANCE")?;
    let config_dir = fsync::get_config_dir()?.join(&instance_name);
    if !config_dir.exists() {
        return Err(Error::Custom(format!(
            "No such config directory: {config_dir}"
        )));
    }
    println!("Found config directory: {config_dir}");

    let config_path = config_dir.join("config.json");
    let config_json = tokio::fs::read(&config_path).await?;
    let config_json = std::str::from_utf8(&config_json)?;
    let config: Config = serde_json::from_str(config_json)?;

    println!("Loaded config: {config:?}");

    let ignored = Arc::new(PatternList::default());

    let local = Arc::new(fsync::backend::fs::Storage::new(&config.local_dir));
    let local = Cache::new_from_storage(local, ignored.clone());

    let tree = match &config.provider {
        Provider::GoogleDrive => {
            let remote = Arc::new(
                fsync::backend::gdrive::Storage::new(oauth2::CacheDir::new(config_dir)).await?,
            );
            let remote = Cache::new_from_storage(remote, ignored);
            let (local, remote) = tokio::join!(local, remote);
            let local = local?;
            let remote = remote?;

            DiffTree::from_cache(&local, &remote)
        }
    };

    let service = Service::new(tree);

    service.start().await
}
