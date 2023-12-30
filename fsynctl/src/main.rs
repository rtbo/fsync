#![feature(iterator_try_collect)]

use std::process;

use clap::Parser;

mod entry;
mod list;
mod new;
mod sync;
mod tree;
mod utils;

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

async fn main2(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::List => list::main(),
        Commands::New(args) => new::main(args),
        Commands::Entry(args) => entry::main(args).await,
        Commands::Tree(args) => tree::main(args).await,
        Commands::Sync(args) => sync::main(args).await,
    }
}
