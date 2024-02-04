use std::process;

use clap::Parser;

mod conflicts;
mod entry;
mod list;
mod nav;
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
    /// Navigate in the repository
    Nav(nav::Args),
    /// Create a new synchronization service
    New(new::Args),
    /// Get the status of an entry
    Entry(entry::Args),
    /// Print the tree status
    Tree(tree::Args),
    /// Synchronize local and remote
    Sync(sync::Args),
    /// List conflicts
    Conflicts(conflicts::Args),
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
        Commands::Nav(args) => nav::main(args).await,
        Commands::New(args) => new::main(args).await,
        Commands::Entry(args) => entry::main(args).await,
        Commands::Tree(args) => tree::main(args).await,
        Commands::Sync(args) => sync::main(args).await,
        Commands::Conflicts(args) => conflicts::main(args).await,
    }
}
