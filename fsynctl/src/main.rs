#![feature(iterator_try_collect)]

use std::process;

use clap::Parser;
use inquire::InquireError;

mod entry;
mod list;
mod new;
mod sync;
mod tree;
mod utils;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Inquire Error")]
    Inquire(#[from] InquireError),

    #[error("IO Error")]
    Io(#[from] std::io::Error),

    #[error("Var Error")]
    Var(#[from] std::env::VarError),

    #[error("Utf-8 error")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("Serde JSON")]
    SerdeJson(#[from] serde_json::Error),

    #[error("Rpc")]
    Rpc(#[from] tarpc::client::RpcError),

    #[error("OAuth2")]
    OAuth2(#[from] yup_oauth2::Error),

    #[error("Custom")]
    Custom(String),

    #[error("Error from deamon: {0}")]
    Deamon(String),
}

impl From<fsync::Error> for Error {
    fn from(error: fsync::Error) -> Self {
        match error {
            fsync::Error::Io(err) => Error::Io(err),
            fsync::Error::Var(err) => Error::Var(err),
            fsync::Error::SerdeJson(err) => Error::SerdeJson(err),
            fsync::Error::Utf8(err) => Error::Utf8(err),
            fsync::Error::OAuth2(err) => Error::OAuth2(err),
            fsync::Error::Custom(err) => Error::Custom(err),
            err => panic!("not supposed to have this error here: {err:?}"),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Parser)]
#[command(name = "fsynctl")]
#[command(author, version, about, long_about=None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// List all installed services
    List,
    /// Create a new synchronization service
    New(new::Args),
    /// Get the status of an entry
    Entry(entry::Args),
    /// Print the tree status
    Tree(tree::Args),
    /// Synchronize local and remote
    Sync(sync::Args),
}

#[tokio::main]
async fn main() -> process::ExitCode {
    let cli = Cli::parse();
    match main2(cli).await {
        Ok(()) => process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            process::ExitCode::FAILURE
        }
    }
}

async fn main2(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::List => list::main(),
        Commands::New(args) => new::main(args),
        Commands::Entry(args) => entry::main(args).await,
        Commands::Tree(args) => tree::main(args).await,
        Commands::Sync(args) => sync::main(args).await,
    }
}
