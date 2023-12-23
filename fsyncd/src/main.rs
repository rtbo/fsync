use std::sync::Arc;

use fsync::cache::CacheStorage;
use fsync::difftree::DiffTree;
use fsync::{oauth2, Config, Error, Provider};
use futures::stream::AbortHandle;
use service::Service;

mod service;

#[tokio::main]
async fn main() -> fsync::Result<()> {
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

    let local = Arc::new(fsync::backend::fs::Storage::new(&config.local_dir));

    let tree = match &config.provider {
        Provider::GoogleDrive => {
            let remote =
                fsync::backend::gdrive::Storage::new(oauth2::CacheDir::new(config_dir)).await?;
            let remote = Arc::new(CacheStorage::new(remote));
            remote.populate().await?;

            DiffTree::from_cache(local, remote).await?
        }
    };

    let service = Service::new(tree);

    let (abort_handle, abort_reg) = AbortHandle::new_pair();
    handle_signals(service.clone(), abort_handle)?;

    service.start(abort_reg).await
}

fn handle_signals(service: Service, abort_handle: AbortHandle) -> fsync::Result<()> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
        };
        service.shutdown();
        abort_handle.abort();
        println!("exiting");
    });

    Ok(())
}
