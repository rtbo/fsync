use std::net::{IpAddr, Ipv6Addr};

use fsync::{path::PathBuf, tree, Conflict, FsyncClient};
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
        tree::Entry::Sync {
            local,
            remote,
            conflict,
        } => {
            assert_eq!(local.path(), remote.path());
            let path = local.path();
            match conflict {
                None => {
                    println!("S {path}")
                }
                Some(Conflict::LocalBigger) => {
                    println!("C {path:<40} local is bigger than remote")
                }
                Some(Conflict::LocalSmaller) => {
                    println!("C {path:<40} local is smaller than remote")
                }
                Some(Conflict::LocalNewer) => println!("C {path:<40} local is newer than remote"),
                Some(Conflict::LocalOlder) => println!("C {path:<40} local is older than remote"),
                Some(Conflict::LocalDirRemoteFile) => {
                    println!("C {path:<40} local is a directory and remote a file")
                }
                Some(Conflict::LocalFileRemoteDir) => {
                    println!("C {path:<40} local is a file and remote a directory")
                }
            }
        }
    }

    Ok(())
}
