use std::str;

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::{cipher, oauth2};

#[derive(Debug, Serialize, Deserialize)]
pub enum Provider {
    GoogleDrive,
}

#[derive(Debug, Clone)]
pub enum AppSecretOpts {
    /// Use built-in google-drive app
    Fsync,
    /// Use custom google-drive app (path to client_secret.json)
    JsonPath(Utf8PathBuf),
    /// Use custom google-drive app (content of client_secret.json)
    JsonContent(String),
    /// Use custom google-drive app (client credentials)
    Credentials {
        client_id: String,
        client_secret: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("I/O error")]
    Io(#[from] std::io::Error),
}

#[test]
fn test_get_appsecret() -> crate::Result<()> {
    let appsecret = AppSecretOpts::Fsync.get()?;
    assert_eq!(appsecret.token_uri, "https://oauth2.googleapis.com/token");
    assert_eq!(
        appsecret.auth_uri,
        "https://accounts.google.com/o/oauth2/auth"
    );
    assert_eq!(appsecret.redirect_uris, ["http://localhost"]);
    assert_eq!(
        appsecret.auth_provider_x509_cert_url,
        Some("https://www.googleapis.com/oauth2/v1/certs".into())
    );
    Ok(())
}

impl AppSecretOpts {
    pub fn get(self) -> crate::Result<oauth2::ApplicationSecret> {
        match self {
            AppSecretOpts::Fsync => {
                const CIPHERED_SECRET: &str = concat!(
                    "nRkHq/y6fB6MxEP+XUpoYuYY3oF3WAYcYEF62twEnls4INPhV/WWVuA5tCw4B8fpHk8nXkMhrQU6g",
                    "WAv9k7MeMa94t2CA1eB3ADhtD1QwteGffKJ/pFxolASh0s8Gs0JdP4RpzgjAAOpRPtrBHgTM6W1It",
                    "UIsQ5mHFSahZyS0obuh9FeXESsetUz0CDQr5l1IG2m4E1c/I790TtLBHut8YDBQs1pNptuaBwDCV7",
                    "DbdXcicbdftiVH9jYd2lt/IvxBi4C7+F8LXS65WGZSYiBrQDb2qkdeasM9tbiGl0/+Yze3ETUA/SN",
                    "urji8/o1fGwcygL8mTsp7DkkOxkjHn18N/a5b8MjhZouxfNvBPKC80AgcdLwmdCXVJ0t7OFobpWxz",
                    "3j57A5URFHyhzj1RqUiui9xldG+AhF69op+QEQSPQ7bWrun6gOYaB1vUvwNt0MzzqM/SUaWVEeT54",
                    "UEVHKqTHva+NBsIzFS/dIsiAYNV8OVcuojl8jPVKlqJJGoS1NO8hog6Gk35GXHZKyIJj/vlzsSOoC",
                    "/5i/Qajyl1/nFfJKUsy+qDZbFkdyevN2UVDFW/wCqLoRJj7P09cHyE8QrHDC9JA"
                );
                let secret_json = cipher::decipher_text(CIPHERED_SECRET);
                Ok(serde_json::from_str(&secret_json)?)
            }
            AppSecretOpts::JsonPath(path) => {
                let secret_json = std::fs::read(path)?;
                let secret_json = str::from_utf8(&secret_json)?;
                Ok(yup_oauth2::parse_application_secret(secret_json)?)
            }
            AppSecretOpts::JsonContent(secret_json) => {
                Ok(yup_oauth2::parse_application_secret(secret_json)?)
            }
            AppSecretOpts::Credentials {
                client_id,
                client_secret,
            } => Ok(oauth2::ApplicationSecret {
                client_id,
                client_secret,
                token_uri: "https://oauth2.googleapis.com/token".into(),
                auth_uri: "https://accounts.google.com/o/oauth2/auth".into(),
                redirect_uris: vec!["http://localhost".into()],
                project_id: None,
                client_email: None,
                auth_provider_x509_cert_url: Some(
                    "https://www.googleapis.com/oauth2/v1/certs".into(),
                ),
                client_x509_cert_url: None,
            }),
        }
    }
}
