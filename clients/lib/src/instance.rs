use std::net::{IpAddr, Ipv6Addr};

use fsync::FsyncClient;
use tarpc::{client, tokio_serde::formats::Bincode};

#[derive(Debug, Clone)]
pub struct Instance {
    name: String,
    port: Option<u16>,
}

impl Instance {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_running(&self) -> bool {
        self.port.is_some()
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn get_all() -> anyhow::Result<Vec<Instance>> {
        use fsync::loc;

        let config_dir = loc::user::config_dir()?;
        if !config_dir.exists() {
            return Ok(vec![]);
        }

        let mut instances = vec![];

        for entry in config_dir.read_dir_utf8()? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_owned();
            let mut port = None;
            let port_path = loc::inst::runtime_port_file(&name)?;
            if port_path.exists() && port_path.is_file() {
                let content = std::fs::read(&port_path)?;
                let content = String::from_utf8(content)?;
                port = Some(str::parse(&content)?);
            }
            instances.push(Instance { name, port });
        }

        Ok(instances)
    }

    /// Make a client for this instance.
    ///
    /// # Panics
    /// Panic if this instance is not running.
    pub async fn make_client(&self) -> anyhow::Result<FsyncClient> {
        let port = self.port.expect("This instance should be running");
        let addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), port);
        let mut transport = tarpc::serde_transport::tcp::connect(addr, Bincode::default);
        transport.config_mut().max_frame_length(usize::MAX);

        Ok(FsyncClient::new(client::Config::default(), transport.await?).spawn())
    }

    pub fn into_name(self) -> String {
        self.name
    }
}
