use fsync::loc::{inst, user};
use fsync::path::{FsPath, FsPathBuf};
use inquire::validator::{ErrorMessage, Validation};
use inquire::{Confirm, CustomUserError, Select, Text};

mod google_drive;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the share
    #[clap(long, short = 'n')]
    name: Option<String>,

    /// The directory to synchronize on the local file system
    #[clap(long, short = 'p')]
    local_dir: Option<FsPathBuf>,
}

pub async fn main(args: Args) -> anyhow::Result<()> {
    for (key, value) in std::env::vars() {
        println!("{key}  =  {value}");
    }
    let name = if let Some(name) = args.name {
        map_validation_result(validate_name(name.as_str()))?;
        name
    } else {
        Text::new("Name of the share?")
            .with_default("drive")
            .with_validator(validate_name)
            .prompt()?
    };

    let config_dir = inst::config_dir(&name)?;
    if config_dir.exists() {
        anyhow::bail!("Configuration already exists: {config_dir}");
    }

    let local_dir = if let Some(local_dir) = args.local_dir {
        map_validation_result(validate_path(local_dir.as_str()))?;
        local_dir
    } else {
        let def = user::home_dir()?.join(&name);
        Text::new("Local directory path?")
            .with_default(def.as_str())
            .with_validator(validate_path)
            .prompt()
            .map(FsPathBuf::from)?
    };

    let providers = vec![fsync::Provider::GoogleDrive];
    let provider = tokio::task::spawn_blocking(move || {
        Select::new("Select drive provider", providers).prompt()
    });
    let provider = provider.await.unwrap()?;

    let opts = prompt_provider_opts(provider).await?;

    let create_res = create_config(&name, &local_dir, &opts).await;
    match create_res {
        Ok(()) => {
            println!("Success!");
        }
        Err(_) => {
            if config_dir.exists() {
                println!("Deleting {config_dir} because of error");
                std::fs::remove_dir_all(config_dir)?;
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

enum ProviderOpts {
    GoogleDrive(google_drive::SecretOpts),
}

impl ProviderOpts {
    async fn create_config(&self, instance_name: &str) -> anyhow::Result<()> {
        match self {
            Self::GoogleDrive(opts) => opts.create_config(instance_name).await,
        }
    }
}

impl From<&ProviderOpts> for fsync::Provider {
    fn from(value: &ProviderOpts) -> Self {
        match value {
            ProviderOpts::GoogleDrive(..) => fsync::Provider::GoogleDrive,
        }
    }
}

async fn prompt_provider_opts(provider: fsync::Provider) -> anyhow::Result<ProviderOpts> {
    match provider {
        fsync::Provider::GoogleDrive => google_drive::prompt_opts(),
    }
}

async fn create_config(
    instance_name: &str,
    local_dir: &FsPath,
    opts: &ProviderOpts,
) -> anyhow::Result<()> {
    let config_dir = inst::config_dir(instance_name)?;
    println!("Creating configuration directory: {config_dir}");
    tokio::fs::create_dir_all(config_dir).await?;

    let config = fsync::Config {
        local_dir: local_dir.to_owned(),
        provider: opts.into(),
    };
    let config_json = serde_json::to_string_pretty(&config)?;
    let config_file = inst::config_file(instance_name)?;
    println!("Writing configuration file: {config_file}");
    tokio::fs::write(&config_file, config_json).await?;

    opts.create_config(instance_name).await
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

fn map_error_message(msg: ErrorMessage) -> anyhow::Error {
    match msg {
        ErrorMessage::Default => anyhow::anyhow!("Invalid input"),
        ErrorMessage::Custom(msg) => anyhow::anyhow!("{msg}"),
    }
}
fn map_validation_result(res: anyhow::Result<Validation, CustomUserError>) -> anyhow::Result<()> {
    match res {
        Ok(Validation::Valid) => Ok(()),
        Ok(Validation::Invalid(msg)) => Err(map_error_message(msg)),
        Err(err) => Err(anyhow::anyhow!("{err}")),
    }
}

fn validate_path(input: &str) -> Result<Validation, CustomUserError> {
    let mut invalid_chars = Vec::new();
    for c in input.as_bytes() {
        match *c {
            b'<' => invalid_chars.push("<"),
            b'>' => invalid_chars.push(">"),
            b'|' => invalid_chars.push("|"),
            b'?' => invalid_chars.push("?"),
            b'*' => invalid_chars.push("*"),
            0..=31 => invalid_chars.push("<ctrl>"),

            #[cfg(not(target_os = "windows"))]
            b':' => invalid_chars.push(":"),
            #[cfg(not(target_os = "windows"))]
            b'\\' => invalid_chars.push("\\"),

            _ => (),
        }
    }
    validate_chars(invalid_chars)
}
