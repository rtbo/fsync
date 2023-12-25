use std::net::{IpAddr, Ipv6Addr};

use camino::Utf8PathBuf;
use fsync::ipc::FsyncClient;
use tarpc::{tokio_serde::formats::Bincode, client, context};

use crate::{utils, Error};

#[derive(clap::Args)]
pub struct Args {
    /// Name of the share
    name: Option<String>,
}

pub async fn main(args: Args) -> Result<(), Error> {
    let share_name = match args.name {
        Some(name) => name,
        None => {
            let name = utils::get_single_share()?;
            if let Some(name) = name {
                name
            } else {
                return Err(Error::Custom("Could not find a single share, please pass the <NAME> as command line argument".into()));
            }
        }
    };

    let port = utils::get_share_port(&share_name)?;

    let addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), port);
    let mut transport = tarpc::serde_transport::tcp::connect(addr, Bincode::default);
    transport.config_mut().max_frame_length(usize::MAX);

    let client = FsyncClient::new(client::Config::default(), transport.await?).spawn();
    let entry = client.entry(context::current(), Utf8PathBuf::from("Musique")).await.unwrap();

    println!("{entry:#?}");

    Ok(())
}
