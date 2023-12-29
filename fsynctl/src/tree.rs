use std::{
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
};

use camino::Utf8PathBuf;
use fsync::{ipc::FsyncClient, tree};
use futures::future::{self, BoxFuture};
use tarpc::{client, context, tokio_serde::formats::Bincode};

use crate::{utils, Error};

#[derive(clap::Args)]
pub struct Args {
    /// Name of the fsyncd instance
    #[clap(long, short = 'n')]
    instance_name: Option<String>,

    /// Path to the entry (root if not specified)
    path: Option<Utf8PathBuf>,
}

pub async fn main(args: Args) -> crate::Result<()> {
    let instance_name = match args.instance_name {
        Some(name) => name,
        None => {
            let name = utils::single_instance_name()?;
            if let Some(name) = name {
                name
            } else {
                return Err(Error::Custom("Could not find a single share, please specify --share-name command line argument".into()));
            }
        }
    };

    let port = utils::instance_port(&instance_name)?;

    let addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), port);
    let mut transport = tarpc::serde_transport::tcp::connect(addr, Bincode::default);
    transport.config_mut().max_frame_length(usize::MAX);

    let client = Arc::new(FsyncClient::new(client::Config::default(), transport.await?).spawn());
    let node = client.entry(context::current(), args.path.clone()).await?;

    let is_root = args.path.is_none();

    if node.is_none() {
        println!("No such entry: {}", args.path.unwrap_or("(root)".into()));
        return Ok(());
    }

    let node = node.unwrap();
    if !is_root {
        print_entry_status(true, !node.children().is_empty(), "", node.entry());
    }

    walk(client, "".into(), node).await
}

// all special unicode are from "box drawing" block starting at \u{2500}

fn walk(
    client: Arc<FsyncClient>,
    prefix: String,
    node: tree::Node,
) -> BoxFuture<'static, crate::Result<()>> {
    Box::pin(async move {
        let dir = node.path();
        let joinvec: Vec<_> = node
            .children()
            .iter()
            .map(|c| client.entry(context::current(), Some(dir.join(c))))
            .collect();
        let children = future::try_join_all(joinvec).await?;
        let mut len = children.len();

        for child in children {
            len -= 1;
            if child.is_none() {
                continue;
            }
            let child = child.unwrap();
            let has_follower = len != 0;

            print_entry_status(false, has_follower, &prefix, child.entry());

            if !child.children().is_empty() {
                let prefix = if has_follower {
                    format!("{prefix}│  ")
                } else {
                    format!("{prefix}   ")
                };
                walk(client.clone(), prefix, child).await?;
            }
        }
        Ok(())
    })
}

fn print_entry_status(first: bool, has_follower: bool, prefix_head: &str, entry: &tree::Entry) {
    let prefix_tail = match (first, has_follower) {
        (true, _) => "",
        (false, true) => "├─ ",
        (false, false) => "└─ ",
    };

    match entry {
        tree::Entry::Local(entry) => {
            println!("L {prefix_head}{prefix_tail}{}", entry.path());
        }
        tree::Entry::Remote(entry) => {
            println!("R {prefix_head}{prefix_tail}{}", entry.path());
        }
        tree::Entry::Both { local, remote } => {
            assert_eq!(local.path(), remote.path());
            let conflict = match (local.is_dir(), remote.is_dir()) {
                (true, true) => None,
                (false, false) if local.mtime() == remote.mtime() => None,
                (false, false) => {
                    let (older, younger) = if local.mtime() < remote.mtime() {
                        ("local", "remote")
                    } else {
                        ("remote", "local")
                    };
                    Some(format!("{older} is older than {younger}"))
                }
                (true, false) => Some("local is a directory, remote a file".into()),
                (false, true) => Some("local is a file, remote a directory".into()),
            };

            let status = if conflict.is_none() { "S" } else { "C" };

            println!("{status} {prefix_head}{prefix_tail}{}", local.path());

            if let Some(conflict) = conflict {
                let prefix_tail = match (first, has_follower) {
                    (true, _) => "",
                    (false, true) => "│  ",
                    (false, false) => "   ",
                };
                println!("  {prefix_head}{prefix_tail}  └─ 🗲 {conflict} 🗲",);
            }
        }
    }
}