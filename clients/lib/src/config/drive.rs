use fsync::{
    oauth2,
    path::{FsPathBuf, PathBuf},
};
use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

use crate::cipher;

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename = "DriveSecretOpts")]
#[serde(rename_all = "camelCase")]
pub enum SecretOpts {
    /// Use built-in google-drive app
    Builtin,

    /// Use custom google-drive app (path to client_secret.json)
    JsonPath(#[type_def(type_of = "String")] FsPathBuf),

    /// Use custom google-drive app (content of client_secret.json)
    JsonContent(String),

    /// Use custom google-drive app (client credentials)
    Credentials {
        client_id: String,
        client_secret: String,
    },
}

#[test]
fn test_get_appsecret() -> anyhow::Result<()> {
    let secret = SecretOpts::Builtin.get()?;
    assert_eq!(
        secret.token_url.as_str(),
        "https://oauth2.googleapis.com/token"
    );
    assert_eq!(
        secret.auth_url.as_str(),
        "https://accounts.google.com/o/oauth2/auth"
    );
    Ok(())
}

impl SecretOpts {
    pub fn get(&self) -> anyhow::Result<oauth2::Secret> {
        match self {
            SecretOpts::Builtin => {
                const CIPHERED_SECRET: &str = concat!(
                    "gRtV+sbymbR9o9QD06bNtV8a+WpfCh223NAjZTTfuMJ+zUBUdzkF1Sr1DCgeAJfYXgd7lt+hww0sK",
                    "bSfB9V26yzgFT4cD/iE+zEbBoPihf/c4A4LKiOxhi/cTubfPdKJFTfFyUzB79vgkcSQqjh79CzEQ/",
                    "KuGgvzpcrOvom93Vn26oOk/XtPNY9AztajbpoOxrt1oHf1mT94Pj/1VOZyoAYIgCgKAuIo3U+YOsm",
                    "HxLepoT6rwdp/9ID+skMnFIotfP5ju8aB/eiU65Z0yKbCaW5Ivnj9nH7klhVW0pbeqKxJgI9RudLR",
                    "N0Y6pFRAFKWXc1/EYQfTrRsa6WRSYMHsj7vJVvedAVE"
                );
                let secret_json = cipher::decipher_text(CIPHERED_SECRET);
                Ok(serde_json::from_str(&secret_json)?)
            }
            SecretOpts::JsonPath(path) => {
                let secret_json = std::fs::read(path)?;
                Ok(serde_json::from_slice(&secret_json)?)
            }
            SecretOpts::JsonContent(secret_json) => Ok(serde_json::from_str(secret_json)?),
            SecretOpts::Credentials {
                client_id,
                client_secret,
            } => Ok(oauth2::Secret {
                client_id: oauth2::ClientId::new(client_id.clone()),
                client_secret: oauth2::ClientSecret::new(client_secret.clone()),
                auth_url: oauth2::AuthUrl::new(
                    "https://accounts.google.com/o/oauth2/auth".to_string(),
                )?,
                token_url: oauth2::TokenUrl::new(
                    "https://oauth2.googleapis.com/token".to_string(),
                )?,
            }),
        }
    }
}

#[tokio::test]
async fn cipher_app_secret() -> anyhow::Result<()> {
    use fsync::path::FsPath;

    let path = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("google_secret.json");
    if path.exists() {
        let output = path.with_file_name("google_secret.cipher.b64");
        let secret = fsync::oauth2::load_google_secret(&path).await?;
        let secret = serde_json::to_string(&secret)?;
        let encoded = cipher::cipher_text(&secret);
        tokio::fs::write(&output, &encoded).await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename = "DriveOpts")]
pub struct Opts {
    pub root: Option<String>,
    pub secret: SecretOpts,
}

impl TryFrom<&Opts> for fsync::config::drive::Config {
    type Error = anyhow::Error;
    fn try_from(value: &Opts) -> Result<Self, Self::Error> {
        let root = value.root.clone();
        let secret = value.secret.get()?;

        Ok(fsync::config::drive::Config {
            root: root.map(PathBuf::from),
            secret,
        })
    }
}
