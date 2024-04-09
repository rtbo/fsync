use fsync::loc::inst;
use fsync::path::{FsPath, FsPathBuf};
use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

pub mod drive;

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
pub enum ProviderOpts {
    #[serde(rename = "drive")]
    GoogleDrive(drive::Opts),

    #[serde(rename = "fs")]
    LocalFs(#[type_def(type_of = "String")] FsPathBuf),
}

impl From<&ProviderOpts> for fsync::Provider {
    fn from(value: &ProviderOpts) -> Self {
        match value {
            ProviderOpts::GoogleDrive(..) => fsync::Provider::GoogleDrive,
            ProviderOpts::LocalFs(..) => fsync::Provider::LocalFs,
        }
    }
}

impl TryFrom<&ProviderOpts> for fsync::ProviderConfig {
    type Error = anyhow::Error;
    fn try_from(value: &ProviderOpts) -> Result<Self, Self::Error> {
        match value {
            ProviderOpts::GoogleDrive(opts) => {
                Ok(fsync::ProviderConfig::GoogleDrive(opts.try_into()?))
            }
            ProviderOpts::LocalFs(path) => Ok(fsync::ProviderConfig::LocalFs(path.clone())),
        }
    }
}

pub async fn create(
    instance_name: &str,
    local_dir: &FsPath,
    opts: &ProviderOpts,
) -> anyhow::Result<()> {
    if instance_name.is_empty() {
        anyhow::bail!("Instance name can't be empty");
    }
    let config_dir = inst::config_dir(instance_name)?;
    println!("Creating configuration directory: {config_dir}");
    tokio::fs::create_dir_all(config_dir).await?;

    let config = fsync::Config {
        local_dir: local_dir.to_owned(),
        provider: opts.try_into()?,
    };
    let config_json = serde_json::to_string_pretty(&config)?;
    let config_file = inst::config_file(instance_name)?;
    println!("Writing configuration file: {config_file}");
    tokio::fs::write(&config_file, config_json).await?;
    Ok(())
}
