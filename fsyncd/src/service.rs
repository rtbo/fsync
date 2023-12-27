use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;

use camino::Utf8PathBuf;
use fsync::ipc::Fsync;
use fsync::loc::inst;
use fsync::tree::{self, DiffTree};
use futures::future;
use futures::prelude::*;
use futures::stream::{AbortRegistration, Abortable};
use tarpc::{
    context::Context,
    server::{self, incoming::Incoming, Channel},
    tokio_serde::formats::Bincode,
};

#[derive(Debug, Clone)]
pub struct Service {
    tree: Arc<DiffTree>,
}

#[tarpc::server]
impl Fsync for Service {
    async fn entry(self, _: Context, path: Option<Utf8PathBuf>) -> Option<tree::Node> {
        self.tree.entry(path.as_deref())
    }
}

impl Service {
    pub fn new(tree: DiffTree) -> Self {
        Self {
            tree: Arc::new(tree),
        }
    }

    pub async fn start(
        &self,
        instance_name: &str,
        abort_reg: AbortRegistration,
    ) -> fsync::Result<()> {
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
