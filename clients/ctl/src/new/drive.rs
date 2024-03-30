use fsync_client::drive::SecretOpts;
use inquire::{Editor, Select, Text};

pub use fsync_client::drive::Opts;

pub fn prompt_opts() -> anyhow::Result<super::ProviderOpts> {
    let root = Text::new("Choose a root in your Google Drive (\"/\" for the entire drive)")
        .with_default("/")
        .prompt_skippable()?;

    let options = &[
        "Use fsync built-in application secret",
        "Provide path to client_secret.json",
        "Paste content of client_secret.json",
        "Enter Google Drive application credentials",
    ];
    let ans = Select::new(
        "Google Drive applidation secret is required",
        options.to_vec(),
    )
    .prompt()?;
    let ind = options.iter().position(|e| *e == ans).unwrap();
    let secret = match ind {
        0 => SecretOpts::Builtin,
        1 => SecretOpts::JsonPath(
            Text::new("Enter path to client_scret.json")
                .prompt()?
                .into(),
        ),
        2 => SecretOpts::JsonContent(Editor::new("Enter content of client_secret.json").prompt()?),
        3 => {
            let client_id = Text::new("Client Id").prompt()?;
            let client_secret = Text::new("Client Secret").prompt()?;
            SecretOpts::Credentials {
                client_id,
                client_secret,
            }
        }
        _ => panic!("Did not recognize answer: {ans}"),
    };

    Ok(super::ProviderOpts::GoogleDrive(Opts { root, secret }))
}
