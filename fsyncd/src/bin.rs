use std::{ffi::OsString, process::ExitCode, sync::Arc};

use clap::Parser;
use fsync::loc::inst;
use fsyncd::{
    oauth2,
    service::{RpcService, Service},
    storage, ShutdownObj,
};
use futures::stream::AbortHandle;
use tokio::sync::RwLock;

#[cfg(not(target_os = "windows"))]
mod posix;

#[cfg(not(target_os = "windows"))]
fn main() -> ExitCode {
    posix::main()
}

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
fn main() -> ExitCode {
    windows::main()
}

#[derive(Clone)]
struct ShutdownRef {
    inner: Arc<RwLock<Option<Arc<dyn ShutdownObj>>>>,
}

impl ShutdownRef {
    fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    async fn set(&self, inner: Arc<dyn ShutdownObj>) {
        let mut write = self.inner.write().await;
        *write = Some(inner);
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        let read = self.inner.read().await;
        match &*read {
            Some(inner) => inner.shutdown_obj().await,
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

    let token_cache_path = &inst::token_cache_file(&cli.instance)?;

    match &config.provider {
        fsync::ProviderConfig::GoogleDrive(config) => {
            log::info!(
                "Initializing Google Drive storage with client-id {}",
                config.secret.client_id.as_str()
            );

            let client = reqwest::Client::builder().build()?;
            let auth = oauth2::Client::new(
                config.secret.clone(),
                oauth2::TokenPersist::MemoryAndDisk(token_cache_path.into()),
                Some(client.clone()),
            )
            .await?;
            let remote =
                storage::gdrive::GoogleDrive::new(auth, client, config.root.as_deref()).await?;
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

    let service = Service::new(local, remote.clone()).await?;
    let service = Arc::new(service);

    shutdown_ref.set(service.clone()).await;

    let (abort_handle, abort_reg) = AbortHandle::new_pair();

    let rpc = RpcService::new(service, abort_handle).await;
    rpc.start(&cli.instance, abort_reg).await
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
