use fsync::{
    loc::{inst, user},
    path::FsPathBuf,
};
use fsync_client::config::ProviderOpts;
use inquire::{
    validator::{ErrorMessage, Validation},
    Confirm, CustomUserError, Select, Text,
};

mod drive;

mod fs {
    use fsync::path::FsPathBuf;
    use inquire::Text;

    pub fn prompt_opts() -> anyhow::Result<super::ProviderOpts> {
        let root = Text::new("Choose the service root in the local file system").prompt()?;
        let root = FsPathBuf::from(root);
        Ok(super::ProviderOpts::LocalFs(root))
    }
}

#[derive(clap::Args)]
pub struct Args {
    /// Name of the share
    name: Option<String>,

    /// The directory to synchronize on the local file system
    #[clap(long, short = 'p')]
    local_dir: Option<FsPathBuf>,
}

pub async fn main(args: Args) -> anyhow::Result<()> {
    let name = if let Some(name) = args.name {
        map_validation_result(validate_name(name.as_str()))?;
        name
    } else {
        Text::new("Name of the share?")
            .with_default("drive")
            .with_validator(validate_name)
            .prompt()?
    };

    println!("Creating new synchronized file service: `{name}`");

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

    let providers = vec![fsync::Provider::GoogleDrive, fsync::Provider::LocalFs];
    let provider = tokio::task::spawn_blocking(move || {
        Select::new("Select remote service provider", providers).prompt()
    });
    let provider = provider.await.unwrap()?;

    let opts = prompt_provider_opts(provider).await?;

    let create_res = fsync_client::config::create(&name, &local_dir, &opts).await;
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

async fn prompt_provider_opts(provider: fsync::Provider) -> anyhow::Result<ProviderOpts> {
    match provider {
        fsync::Provider::GoogleDrive => drive::prompt_opts(),
        fsync::Provider::LocalFs => fs::prompt_opts(),
    }
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
