use std::sync::Arc;

use fsync::cache::CacheStorage;
use fsync::difftree::DiffTree;
use fsync::{oauth2, Config, Error, Provider};
use futures::stream::AbortHandle;
use futures::Future;
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

    let mut remote = match &config.provider {
        Provider::GoogleDrive => {
            let remote =
                fsync::backend::gdrive::Storage::new(oauth2::CacheDir::new(config_dir)).await?;
            CacheStorage::new(remote)
        }
    };

    let cache_path = fsync::get_instance_cache_dir(&instance_name)?;
    let remote_cache_path = cache_path.join("remote.bin");

    match remote.load_from_disk(&remote_cache_path).await {
        Err(fsync::Error::Io(_)) => {
            remote.populate_from_storage().await?;
        }
        Err(err) => Err(err)?,
        Ok(()) => (),
    }
    let remote = Arc::new(remote);

    let tree = DiffTree::from_cache(local, remote.clone()).await?;
    let service = Service::new(tree);

    let abort_reg = {
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        let service = service.clone();
        handle_shutdown_signals(|| async move {
            tokio::fs::create_dir_all(cache_path).await.unwrap();
            remote.save_to_disc(&remote_cache_path).await.unwrap();
            service.shutdown();
            abort_handle.abort();
        })?;
        abort_reg
    };

    service.start(abort_reg).await
}

fn handle_shutdown_signals<F, Fut>(shutdown: F) -> fsync::Result<()>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
        };
        shutdown().await;
        println!("exiting");
    });

    Ok(())
}
