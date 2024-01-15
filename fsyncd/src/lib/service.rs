use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;

use fsync::{
    self,
    loc::inst,
    path::{Path, PathBuf},
    Fsync,
};
use futures::future;
use futures::prelude::*;
use futures::stream::{AbortHandle, AbortRegistration, Abortable};
use tarpc::{
    context::Context,
    server::{self, incoming::Incoming, Channel},
    tokio_serde::formats::Bincode,
};
use tokio::sync::RwLock;

use crate::storage;
use crate::tree::{self, DiffTree};

#[derive(Debug)]
pub struct Service<L, R> {
    local: L,
    remote: R,
    tree: DiffTree,
    abort_handle: RwLock<Option<AbortHandle>>,
}

impl<L, R> Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    pub async fn new(local: L, remote: R) -> anyhow::Result<Self> {
        let local = Arc::new(local);
        let remote = Arc::new(remote);
        let tree = DiffTree::build(local.clone(), remote.clone()).await?;
        let local = Arc::into_inner(local).unwrap();
        let remote = Arc::into_inner(remote).unwrap();

        Ok(Self {
            local,
            remote,
            tree,
            abort_handle: RwLock::new(None),
        })
    }

    pub fn local(&self) -> &L {
        &self.local
    }

    pub fn remote(&self) -> &R {
        &self.remote
    }
}

fn check_path(path: &Path) -> Result<(), String> {
    if path.is_relative() {
        Err(format!("Expect an absolute path, got '{path}'"))
    } else {
        Ok(())
    }
}

impl<L, R> Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    pub async fn entry(&self, path: &Path) -> Result<Option<fsync::tree::Node>, String> {
        check_path(path)?;
        Ok(self.tree.entry(&path).map(Into::into))
    }

    pub async fn copy_remote_to_local(&self, path: &Path) -> Result<(), String> {
        check_path(path)?;
        let entry = self.tree.entry(&path);
        if entry.is_none() {
            return Err(format!("No such entry in remote drive: '{path}'"));
        }
        let node = entry.unwrap();

        match node.entry() {
            tree::Entry::Local(..) => Err(format!("{path} is on the local drive only")),
            tree::Entry::Remote(remote) => {
                let read = self
                    .remote
                    .read_file(remote.path().to_owned())
                    .await
                    .map_err(|err| err.to_string())?;
                let local = self
                    .local
                    .create_file(remote, read)
                    .await
                    .map_err(|err| err.to_string())?;
                self.tree.add_local(&path, local).unwrap();
                Ok(())
            }
            _ => Err(format!("'{path}' is not only on remote")),
        }
    }

    pub async fn copy_local_to_remote(&self, path: &Path) -> Result<(), String> {
        check_path(path)?;
        let entry = self.tree.entry(&path);
        if entry.is_none() {
            return Err(format!("No such entry in local drive: '{path}'"));
        }
        let node = entry.unwrap();

        match node.entry() {
            tree::Entry::Local(local) => {
                let read = self
                    .local
                    .read_file(local.path().to_owned())
                    .await
                    .map_err(|err| err.to_string())?;

                let remote = self.remote.create_file(local, read).await.unwrap();

                self.tree.add_remote(&path, remote).unwrap();
                Ok(())
            }
            tree::Entry::Remote(..) => Err(format!("'{path}' is on the remote drive only")),
            _ => Err(format!("'{path}' is not only on remote")),
        }
    }
}

impl<L, R> crate::Shutdown for Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    async fn shutdown(&self) -> anyhow::Result<()> {
        log::info!("Shutting service down");
        {
            let abort_handle = self.abort_handle.read().await;
            if let Some(abort_handle) = &*abort_handle {
                abort_handle.abort();
            }
        }
        let fut1 = self.local.shutdown();
        let fut2 = self.remote.shutdown();
        tokio::try_join!(fut1, fut2)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct RpcService<L, R> {
    inner: Arc<Service<L, R>>,
}

impl<L, R> RpcService<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    pub async fn new(service: Arc<Service<L, R>>, abort_handle: AbortHandle) -> Self {
        debug_assert!(
            service.abort_handle.read().await.is_none(),
            "Cannot share Service among multiple RpcService"
        );
        *service.abort_handle.write().await = Some(abort_handle);
        Self { inner: service }
    }

    pub async fn start(
        &self,
        instance_name: &str,
        abort_reg: AbortRegistration,
    ) -> anyhow::Result<()> {
        let server_addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), 0);

        let mut listener =
            tarpc::serde_transport::tcp::listen(&server_addr, Bincode::default).await?;

        log::info!("Listening on port {}", listener.local_addr().port());

        let port_path = inst::runtime_port_file(instance_name)?;
        tokio::fs::create_dir_all(port_path.parent().unwrap()).await?;

        let port_str = serde_json::to_string(&listener.local_addr().port())?;
        log::trace!("Creating file {port_path}");
        tokio::fs::write(&port_path, port_str.as_bytes()).await?;

        listener.config_mut().max_frame_length(usize::MAX);
        let fut = listener
            // Ignore accept errors.
            .filter_map(|r| future::ready(r.ok()))
            .map(server::BaseChannel::with_defaults)
            // Limit channels to 1 per IP.
            .max_channels_per_key(1, |t| t.transport().peer_addr().unwrap().ip())
            // serve is generated by the service attribute. It takes as input any type implementing
            // the generated Fsync trait.
            .map(|channel| channel.execute(self.clone().serve()))
            // Max 10 channels.
            .buffer_unordered(10)
            .for_each(|_| async {});

        let _ = Abortable::new(fut, abort_reg).await;

        log::trace!("Removing file {port_path}");
        tokio::fs::remove_file(&port_path).await?;
        Ok(())
    }
}

#[tarpc::server]
impl<L, R> Fsync for RpcService<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    async fn entry(self, _: Context, path: PathBuf) -> Result<Option<fsync::tree::Node>, String> {
        let res = self.inner.entry(&path).await;
        log::trace!(target: "RPC", "Fsync::entry(path: {path:?}) -> {res:#?}");
        res
    }

    async fn copy_remote_to_local(self, _: Context, path: PathBuf) -> Result<(), String> {
        let res = self.inner.copy_remote_to_local(&path).await;
        log::trace!(target: "RPC", "Fsync::copy_remote_to_local(path: {path:?}) -> {res:#?}");
        res
    }

    async fn copy_local_to_remote(self, _: Context, path: PathBuf) -> Result<(), String> {
        let res = self.inner.copy_local_to_remote(&path).await;
        log::trace!(target: "RPC", "Fsync::copy_local_to_remote(path: {path:?}) -> {res:#?}");
        res
    }
}
