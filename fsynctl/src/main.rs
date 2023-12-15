
use clap::Parser;

mod new;

#[derive(Parser)]
#[command(name = "fsynctl")]
#[command(author, version, about, long_about=None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Create a new synchronization service
    New(new::Args),
}

fn main() {
    let cli = Cli::parse();
}
