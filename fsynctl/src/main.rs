
use clap::Parser;
use inquire::InquireError;

mod list;
mod new;

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

    #[error("OAuth2")]
    OAuth2(#[from] yup_oauth2::Error),

    #[error("Custom")]
    Custom(String),
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
            err => panic!("not supposed to have this error here: {err:?}")
        }
    }
}

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
}

fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    match cli.command {
        Commands::List => list::main(),
        Commands::New(args) => new::main(args),
    }
}
