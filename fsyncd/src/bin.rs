use clap::Parser;
use fsync::{loc::inst, oauth};
use fsyncd_lib::{service, storage};
use futures::stream::AbortHandle;
use futures::Future;

#[derive(Parser)]
#[command(name = "fsyncd")]
#[command(author, version, about, long_about=None)]
struct Cli {
    instance: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config_file = inst::config_file(&cli.instance)?;

    if !&config_file.exists() {
        anyhow::bail!("No such config file: {config_file}");
    }
    println!("Found config file: {config_file}");

    let config = fsync::Config::load_from_file(&config_file).await?;
    println!("Loaded config: {config:?}");

    let local = storage::fs::Storage::new(&config.local_dir);

    let secret = {
        let path = inst::oauth_secret_file(&cli.instance)?;
        let json = tokio::fs::read(&path).await?;
        serde_json::from_slice(&json)?
    };
    let token_cache_path = &inst::token_cache_file(&cli.instance)?;
    let oauth2_params = oauth::Params {
        secret,
        token_cache_path,
    };

    match &config.provider {
        fsync::Provider::GoogleDrive => {
            let remote = storage::gdrive::GoogleDrive::new(oauth2_params).await?;
            start_service(cli, local, remote).await
        }
    }
}

async fn start_service<L, R>(cli: Cli, local: L, remote: R) -> anyhow::Result<()>
where
    L: storage::Storage,
    R: storage::id::Storage,
{
    let remote_cache_path = inst::remote_cache_file(&cli.instance)?;
    tokio::fs::create_dir_all(remote_cache_path.parent().unwrap())
        .await
        .unwrap();

    let mut remote = storage::cache::CacheStorage::new(remote, remote_cache_path);
    if remote.load_from_disk().await.is_err() {
        remote.populate_from_entries().await?;
    }

    let service = service::Service::new(local, remote.clone()).await?;

    let abort_reg = {
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        let service = service.clone();
        handle_shutdown_signals(|| async move {
            service.shutdown().await;
            abort_handle.abort();
        })?;
        abort_reg
    };

    service.start(&cli.instance, abort_reg).await
}

fn handle_shutdown_signals<F, Fut>(shutdown: F) -> anyhow::Result<()>
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
