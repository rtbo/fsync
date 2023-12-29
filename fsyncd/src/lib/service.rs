use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;

use camino::Utf8PathBuf;
use fsync::{self, Fsync, tree, loc::inst};
use futures::future;
use futures::prelude::*;
use futures::stream::{AbortRegistration, Abortable};
use tarpc::{
    context::Context,
    server::{self, incoming::Incoming, Channel},
    tokio_serde::formats::Bincode,
};


use crate::tree::DiffTree;
use crate::storage;

#[derive(Debug, Clone)]
pub struct Service<L, R> {
    local: Arc<L>,
    remote: Arc<R>,
    tree: Arc<DiffTree>,
}

impl<L, R> Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    pub async fn new(local: L, remote: R) -> anyhow::Result<Self> {
        let local = Arc::new(local);
        let remote = Arc::new(remote);
        let tree = DiffTree::from_cache(local.clone(), remote.clone()).await?;
        Ok(Self {
            local,
            remote,
            tree: Arc::new(tree),
        })
    }

    pub async fn start(
        &self,
        instance_name: &str,
        abort_reg: AbortRegistration,
    ) -> anyhow::Result<()> {
        let server_addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), 0);

        let mut listener =
            tarpc::serde_transport::tcp::listen(&server_addr, Bincode::default).await?;

        println!("Listening on port {}", listener.local_addr().port());

        let port_path = inst::runtime_port_file(instance_name)?;
        tokio::fs::create_dir_all(port_path.parent().unwrap()).await?;

        let port_str = serde_json::to_string(&listener.local_addr().port())?;
        println!("Creating file {port_path}");
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

        println!("Removing file {port_path}");
        tokio::fs::remove_file(&port_path).await?;
        println!("Exiting server");
        Ok(())
    }

    pub fn shutdown(&self) {}
}

#[tarpc::server]
impl<L, R> Fsync for Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    async fn entry(self, _: Context, path: Option<Utf8PathBuf>) -> Option<tree::Node> {
        self.tree.entry(path.as_deref())
    }

    async fn copy_remote_to_local(self, _: Context, path: Utf8PathBuf) -> Result<(), String> {
        let entry = self.tree.entry(Some(&path));
        if entry.is_none() {
            return Err(format!("no such entry in remote drive: {path}"));
        }
        let node = entry.unwrap();

        match node.entry() {
            tree::Entry::Remote(remote) => {
                let read = self
                    .remote
                    .read_file(remote.path_id())
                    .await
                    .map_err(|err| err.to_string())?;
                let local = self
                    .local
                    .create_file(&remote, read)
                    .await
                    .map_err(|err| err.to_string())?;
                self.tree.add_local(&path, local).unwrap();
                Ok(())
            }
            _ => Err(format!("{path} is not only on remote")),
        }
    }
}
