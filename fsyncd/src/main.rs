use std::sync::Arc;

use clap::Parser;
use fsync::cache::CacheStorage;
use fsync::difftree::DiffTree;
use fsync::{self, oauth2};
use futures::stream::AbortHandle;
use futures::Future;
use service::Service;

mod service;

#[derive(Parser)]
#[command(name = "fsyncd")]
#[command(author, version, about, long_about=None)]
struct Cli {
    instance: String,
}

#[tokio::main]
async fn main() -> fsync::Result<()> {
    let cli = Cli::parse();

    let config_path = fsync::instance_config_dir(&cli.instance)?;
    if !config_path.exists() {
        return Err(fsync::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("No such config directory: {config_path}"),
        )));
    }
    println!("Found config directory: {config_path}");

    let config = fsync::Config::load_from_file(&config_path.join("config.json")).await?;
    println!("Loaded config: {config:?}");

    let local = Arc::new(fsync::backend::fs::Storage::new(&config.local_dir));

    let cache_path = fsync::instance_cache_dir(&cli.instance)?;
    let oauth_files = oauth2::Files::new(config_path.join("client_secret.json"), cache_path.join("token_cache.json"));

    let mut remote = match &config.provider {
        fsync::Provider::GoogleDrive => {
            let remote =
                fsync::backend::gdrive::Storage::new(oauth_files).await?;
            CacheStorage::new(remote)
        }
    };

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
