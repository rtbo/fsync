
use camino::{Utf8Path, Utf8PathBuf};
use inquire::validator::{ErrorMessage, Validation};
use inquire::{Confirm, CustomUserError, Editor, Select, Text};

use crate::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the share
    #[clap(long, short = 'n')]
    name: Option<String>,

    /// The directory to synchronize on the local file system
    #[clap(long, short = 'p')]
    local_dir: Option<Utf8PathBuf>,
}

struct InitOptions {
    local_dir: Utf8PathBuf,
    provider_opts: ProviderOpts,
}

enum ProviderOpts {
    GoogleDrive(gdrive::AppSecretOpts),
}

pub fn main(args: Args) -> Result<(), Error> {
    let name = if let Some(name) = args.name {
        map_validation_result(validate_name(name.as_str()))?;
        name
    } else {
        Text::new("Name of the share?")
            .with_default("drive")
            .with_validator(validate_name)
            .prompt()?
    };

    let config_dir = fsync::get_config_dir()?.join(&name);
    if config_dir.exists() {
        return Err(Error::Custom(format!("Configuration already exists: {config_dir}")));
    }

    let local_dir = if let Some(local_dir) = args.local_dir {
        map_validation_result(validate_path(local_dir.as_str()))?;
        local_dir
    } else {
        let def = fsync::get_home()?.join(&name);
        Text::new("Local directory path?")
            .with_default(def.as_str())
            .with_validator(validate_path)
            .prompt()
            .map(|ld| Utf8PathBuf::from(ld))?
    };

    const GOOGLE_DRIVE_PROVIDER: &'static str = "Google Drive";

    let providers = vec![GOOGLE_DRIVE_PROVIDER];
    let provider = Select::new("Select drive provider", providers).prompt()?;

    let provider = if provider == GOOGLE_DRIVE_PROVIDER {
        ProviderOpts::GoogleDrive(google_drive()?)
    } else {
        panic!("Could not recognize answer: {provider}");
    };

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let create_res = rt.block_on(create_config(
        &config_dir,
        InitOptions {
            local_dir: local_dir.clone(),
            provider_opts: provider,
        },
    ));
    match create_res {
        Ok(()) => {
            println!("Success!");
        },
        Err(_) => {
            if config_dir.exists() {
                println!("Deleting {config_dir} because of error");
                std::fs::remove_dir_all(&config_dir)?;
            }
        }
    }

    if !local_dir.exists() {
        let message = format!("Create directory {local_dir}?");
        let ans = Confirm::new(&message).with_default(true).prompt()?;
        if ans {
            std::fs::create_dir_all(local_dir.as_path())?;
        }
    }

    Ok(())
}

async fn create_config(config_dir: &Utf8Path, opts: InitOptions) -> Result<(), Error> {
    println!("Creating configuration directory: {}", config_dir);
    tokio::fs::create_dir_all(&config_dir).await?;

    let config = fsync::Config {
        local_dir: opts.local_dir.clone().into(),
        provider: match opts.provider_opts {
            ProviderOpts::GoogleDrive(..) => fsync::Provider::GoogleDrive,
        },
    };
    let config_json = serde_json::to_string_pretty(&config)?;
    let config_path = config_dir.join("config.json");
    println!("Writing configuration file: {config_path}");
    tokio::fs::write(&config_path, config_json).await?;

    match opts.provider_opts {
        ProviderOpts::GoogleDrive(app_secret_opts) => {
            let app_secret =  app_secret_opts.get()?;
            let cache_dir = gdrive::CacheDir::new(config_dir.to_path_buf());
            cache_dir.cache_secret(&app_secret).await?;
            cache_dir.auth_and_cache_tokens(app_secret).await?;
        }
    }

    Ok(())
}

fn validate_chars(mut invalid_chars: Vec<&str>) -> Result<Validation, CustomUserError> {
    invalid_chars.sort_unstable();
    invalid_chars.dedup();
    if invalid_chars.is_empty() {
        Ok(Validation::Valid)
    } else {
        let invalid_chars = invalid_chars.join(", ");
        Ok(Validation::Invalid(ErrorMessage::Custom(format!(
            "invalid characters: {invalid_chars}"
        ))))
    }
}

fn validate_name(input: &str) -> Result<Validation, CustomUserError> {
    let mut invalid_chars = Vec::new();
    for c in input.as_bytes() {
        match *c {
            b'/' => invalid_chars.push("/"),
            b'\\' => invalid_chars.push("\\"),
            b'<' => invalid_chars.push("<"),
            b'>' => invalid_chars.push(">"),
            b':' => invalid_chars.push(":"),
            b'|' => invalid_chars.push("|"),
            b'?' => invalid_chars.push("?"),
            b'*' => invalid_chars.push("*"),
            0..=31 => invalid_chars.push("<ctrl>"),
            _ => (),
        }
    }
    validate_chars(invalid_chars)
}

fn map_error_message(msg: ErrorMessage) -> Error {
    match msg {
        ErrorMessage::Default => Error::Custom("Invalid input".into()),
        ErrorMessage::Custom(msg) => Error::Custom(msg),
    }
}
fn map_validation_result(res: Result<Validation, CustomUserError>) -> Result<(), Error> {
    match res {
        Ok(Validation::Valid) => Ok(()),
        Ok(Validation::Invalid(msg)) => Err(map_error_message(msg)),
        Err(err) => Err(Error::Custom(err.to_string())),
    }
}

fn validate_path(input: &str) -> Result<Validation, CustomUserError> {
    let mut invalid_chars = Vec::new();
    for c in input.as_bytes() {
        match *c {
            b'<' => invalid_chars.push("<"),
            b'>' => invalid_chars.push(">"),
            b':' => invalid_chars.push(":"),
            b'|' => invalid_chars.push("|"),
            b'?' => invalid_chars.push("?"),
            b'*' => invalid_chars.push("*"),
            0..=31 => invalid_chars.push("<ctrl>"),

            #[cfg(not(target_os = "windows"))]
            b'\\' => invalid_chars.push("\\"),

            _ => (),
        }
    }
    validate_chars(invalid_chars)
}

fn google_drive() -> Result<gdrive::AppSecretOpts, Error> {
    let options = &[
        "Use built-in application secret",
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
    let opts = match ind {
        0 => gdrive::AppSecretOpts::Fsync,
        1 => gdrive::AppSecretOpts::JsonPath(
            Text::new("Enter path to client_scret.json")
                .prompt()?
                .into(),
        ),
        2 => gdrive::AppSecretOpts::JsonContent(
            Editor::new("Enter content of client_secret.json").prompt()?,
        ),
        3 => {
            let client_id = Text::new("Client Id").prompt()?;
            let client_secret = Text::new("Client Secret").prompt()?;
            gdrive::AppSecretOpts::Credentials {
                client_id,
                client_secret,
            }
        }
        _ => panic!("Did not recognize answer: {ans}"),
    };
    Ok(opts)
}
