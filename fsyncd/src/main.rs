use std::sync::Arc;

use clap::Parser;
use fsync::cache::CacheStorage;
use fsync::tree::DiffTree;
use fsync::{self, backend, loc::inst};
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

    let config_file = inst::config_file(&cli.instance)?;
    if !&config_file.exists() {
        return Err(fsync::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("No such config file: {config_file}"),
        )));
    }
    println!("Found config file: {config_file}");

    let config = fsync::Config::load_from_file(&config_file).await?;
    println!("Loaded config: {config:?}");

    let local = fsync::backend::fs::Storage::new(&config.local_dir);

    match &config.provider {
        fsync::Provider::GoogleDrive => {
            let remote = backend::gdrive::Storage::new(
                &inst::oauth_secret_file(&cli.instance)?,
                &inst::token_cache_file(&cli.instance)?,
            )
            .await?;
            start_service(cli, local, remote).await
        }
    }
}

async fn start_service<L, R>(cli: Cli, local: L, remote: R) -> fsync::Result<()>
where
    L: fsync::Storage,
    R: fsync::Storage,
{
    let remote_cache_path = inst::remote_cache_file(&cli.instance)?;
    let mut remote = CacheStorage::new(remote);
    match remote.load_from_disk(&remote_cache_path).await {
        Err(fsync::Error::Io(_)) => {
            remote.populate_from_entries().await?;
        }
        Err(err) => Err(err)?,
        Ok(()) => (),
    }

    let local = Arc::new(local);
    let remote = Arc::new(remote);

    let tree = DiffTree::from_cache(local, remote.clone()).await?;

    let service = Service::new(tree);

    let abort_reg = {
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        let service = service.clone();
        handle_shutdown_signals(|| async move {
            tokio::fs::create_dir_all(remote_cache_path.parent().unwrap())
                .await
                .unwrap();
            remote.save_to_disc(&remote_cache_path).await.unwrap();
            service.shutdown();
            abort_handle.abort();
        })?;
        abort_reg
    };

    service.start(&cli.instance, abort_reg).await
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
    });

    Ok(())
}
