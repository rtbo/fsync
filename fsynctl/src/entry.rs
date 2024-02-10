use std::{
    cmp::Ordering,
    net::{IpAddr, Ipv6Addr},
};

use fsync::{path::PathBuf, tree, FsyncClient};
use tarpc::{client, context, tokio_serde::formats::Bincode};

use crate::utils;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the fsyncd instance
    #[clap(long, short = 'n')]
    instance_name: Option<String>,

    /// Path to the entry
    path: Option<PathBuf>,
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
    let path = args.path.unwrap_or_else(PathBuf::root);
    let entry = client
        .entry(context::current(), path.clone())
        .await
        .unwrap()
        .unwrap();

    if entry.is_none() {
        println!("No such entry: {path}");
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
        tree::Entry::Sync { local, remote } => {
            assert_eq!(local.path(), remote.path());
            let mtime_cmp = fsync::compare_mtime_opt(local.mtime(), remote.mtime());
            match mtime_cmp {
                None | Some(Ordering::Equal) => println!("S {}", local.path()),
                Some(Ordering::Less) => println!("C {:<40} local older than remote", local.path()),
                Some(Ordering::Greater) => {
                    println!("C {:<40} remote older than local", local.path())
                }
            }
        }
    }

    Ok(())
}
