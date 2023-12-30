use std::net::{IpAddr, Ipv6Addr};

use camino::Utf8PathBuf;
use fsync::{FsyncClient, tree};
use tarpc::{client, context, tokio_serde::formats::Bincode};

use crate::utils;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the fsyncd instance
    #[clap(long, short = 'n')]
    instance_name: Option<String>,

    /// Path to the entry
    path: Option<Utf8PathBuf>,
}

pub async fn main(args: Args) -> anyhow::Result<()> {
    let instance_name = match args.instance_name {
        Some(name) => name,
        None => {
            let name = utils::single_instance_name()?;
            if let Some(name) = name {
                name
            } else {
                anyhow::bail!("Could not find a single share, please specify --share-name command line argument");
            }
        }
    };

    let port = utils::instance_port(&instance_name)?;

    let addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), port);
    let mut transport = tarpc::serde_transport::tcp::connect(addr, Bincode::default);
    transport.config_mut().max_frame_length(usize::MAX);

    let client = FsyncClient::new(client::Config::default(), transport.await?).spawn();
    let entry = client
        .entry(context::current(), args.path.clone())
        .await
        .unwrap();

    if entry.is_none() {
        println!("No such entry: {}", args.path.unwrap_or("(root)".into()));
        return Ok(());
    }
    let entry = entry.unwrap();

    match entry.entry() {
        tree::Entry::Local(entry) => {
            println!("L {}", entry.path());
        }
        tree::Entry::Remote(entry) => {
            println!("R {}", entry.path());
        }
        tree::Entry::Both { local, remote } => {
            assert_eq!(local.path(), remote.path());
            if local.mtime() == remote.mtime() {
                println!("S {}", local.path());
            } else {
                let (older, younger) = if local.mtime() < remote.mtime() {
                    ("local", "remote")
                } else {
                    ("remote", "local")
                };
                println!("C {:<40} {older} older than {younger}", local.path());
            }
        }
    }

    Ok(())
}
