use std::{ffi::OsString, process::ExitCode};
use std::sync::Arc;

use clap::Parser;
use fsync::{loc::inst, oauth};
use fsyncd::{service, storage, Shutdown};
use futures::stream::AbortHandle;
use tokio::sync::RwLock;

#[cfg(not(target_os = "windows"))]
mod posix;

#[cfg(target_os = "windows")]
mod windows;

fn main() {
    #[cfg(not(target_os = "windows"))]
    posix::main();

    #[cfg(target_os = "windows")]
    windows::main().unwrap();
}

#[derive(Clone)]
struct ShutdownRef {
    inner: Arc<RwLock<Option<Arc<dyn Shutdown>>>>,
}

impl ShutdownRef {
    fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    async fn set(&self, inner: Arc<dyn Shutdown>) {
        let mut write = self.inner.write().await;
        *write = Some(inner);
    }

    async fn shutdown(&self) -> anyhow::Result<()>{
        let read = self.inner.read().await;
        match &*read {
            Some(shutdown) => shutdown.shutdown().await,
            None => Ok(()),
        }
    }
}

#[derive(Parser)]
#[command(name = "fsyncd")]
#[command(author, version, about, long_about=None)]
struct Cli {
    instance: String,
}

async fn run(args: Vec<OsString>, shutdown_ref: ShutdownRef) -> anyhow::Result<()> {
    let cli = Cli::parse_from(args);

    let config_file = inst::config_file(&cli.instance)?;

    if !&config_file.exists() {
        anyhow::bail!("No such config file: {config_file}");
    }

    log::info!("Found config file: {config_file}");

    let config = fsync::Config::load_from_file(&config_file).await?;
    log::trace!("Loaded config: {config:?}");

    let local = storage::fs::FileSystem::new(&config.local_dir)?;

    let secret_path = inst::oauth_secret_file(&cli.instance)?;
    let secret = {
        let json = tokio::fs::read(&secret_path).await?;
        serde_json::from_slice(&json)?
    };
    log::trace!("Loaded OAuth2 secrets from {secret_path}");

    let token_cache_path = &inst::token_cache_file(&cli.instance)?;
    let oauth2_params = oauth::Params {
        secret,
        token_cache_path,
    };

    match &config.provider {
        fsync::Provider::GoogleDrive => {
            log::info!(
                "Initializing Google Drive storage with client-id {}",
                oauth2_params.secret.client_id.as_str()
            );

            let client = reqwest::Client::builder().build()?;
            let auth = fsyncd::oauth::Client::new(
                oauth2_params.secret,
                fsyncd::oauth::TokenCache::MemoryAndDisk(oauth2_params.token_cache_path.into()),
                Some(client.clone()),
            )
            .await?;
            let remote = storage::gdrive::GoogleDrive::new(auth, client, None).await?;
            start_service(cli, local, remote, shutdown_ref).await
        }
    }
}

async fn start_service<L, R>(
    cli: Cli,
    local: L,
    remote: R,
    shutdown_ref: ShutdownRef,
) -> anyhow::Result<()>
where
    L: storage::Storage,
    R: storage::id::Storage,
{
    let remote_cache_path = inst::remote_cache_file(&cli.instance)?;
    let remote_cache_dir = remote_cache_path.parent().unwrap();
    log::trace!("mkdir -p {remote_cache_dir}");
    tokio::fs::create_dir_all(remote_cache_dir).await.unwrap();

    let mut remote = storage::cache::CacheStorage::new(remote, remote_cache_path);
    if remote.load_from_disk().await.is_err() {
        remote.populate_from_entries().await?;
    }

    let (abort_handle, abort_reg) = AbortHandle::new_pair();

    let service = service::Service::new(local, remote.clone(), abort_handle).await?;
    let service = Arc::new(service);

    shutdown_ref.set(service.clone()).await;

    service.start(&cli.instance, abort_reg).await
}

pub fn exit_program(shutdown_res: anyhow::Result<()>) -> ExitCode {
    match shutdown_res {
        Ok(..) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("Error during fsyncd shutdown: {err:#}");
            ExitCode::FAILURE
        }
    }
}
