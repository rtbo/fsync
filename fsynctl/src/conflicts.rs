use fsync::Conflict;
use tarpc::context;

use crate::utils;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Name of the fsyncd instance
    #[clap(long, short = 'n')]
    instance_name: Option<String>,
}

fn ctx() -> context::Context {
    context::current()
}

pub async fn main(args: Args) -> anyhow::Result<()> {
    let instance_name = match &args.instance_name {
        Some(name) => name.clone(),
        None => {
            let name = utils::single_instance_name()?;
            if let Some(name) = name {
                name
            } else {
                anyhow::bail!("Could not find a single share, please specify --share-name command line argument");
            }
        }
    };

    let client = utils::instance_client(&instance_name).await?;

    let conflicts = client.conflicts(ctx(), None, 100).await.unwrap()?;

    println!("{} conflicts found!", conflicts.len());

    for c in conflicts {
        match c {
            Conflict::LocalBigger { path, .. } => {
                println!("C {path} local is bigger (but same mtime)");
            }
            Conflict::LocalSmaller { path, .. } => {
                println!("C {path} local is smaller (but same mtime)");
            }
            Conflict::LocalNewer { path, .. } => {
                println!("C {path} local is newer");
            }
            Conflict::LocalOlder { path, .. } => {
                println!("C {path} local is older");
            }
            Conflict::LocalFileRemoteDir { path, .. } => {
                println!("C {path} local is file, remote is dir");
            }
            Conflict::LocalDirRemoteFile { path, .. } => {
                println!("C {path} local is dir, remote is file");
            }
        }
    }
    Ok(())
}
