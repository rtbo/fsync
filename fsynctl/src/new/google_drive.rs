use std::str;

use camino::Utf8PathBuf;
use fsync::{cipher, loc::inst, oauth2};
use inquire::{Editor, Select, Text};

pub fn prompt_opts() -> anyhow::Result<super::ProviderOpts> {
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
        0 => AppSecretOpts::Fsync,
        1 => AppSecretOpts::JsonPath(
            Text::new("Enter path to client_scret.json")
                .prompt()?
                .into(),
        ),
        2 => {
            AppSecretOpts::JsonContent(Editor::new("Enter content of client_secret.json").prompt()?)
        }
        3 => {
            let client_id = Text::new("Client Id").prompt()?;
            let client_secret = Text::new("Client Secret").prompt()?;
            AppSecretOpts::Credentials {
                client_id,
                client_secret,
            }
        }
        _ => panic!("Did not recognize answer: {ans}"),
    };
    Ok(super::ProviderOpts::GoogleDrive(opts))
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

#[test]
fn test_get_appsecret() -> anyhow::Result<()> {
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
    pub fn get(&self) -> anyhow::Result<oauth2::ApplicationSecret> {
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
                Ok(oauth2::parse_application_secret(secret_json)?)
            }
            AppSecretOpts::JsonContent(secret_json) => {
                Ok(oauth2::parse_application_secret(secret_json)?)
            }
            AppSecretOpts::Credentials {
                client_id,
                client_secret,
            } => Ok(oauth2::ApplicationSecret {
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
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

    pub async fn create_config(&self, instance_name: &str) -> anyhow::Result<()> {
        let app_secret = self.get()?;
        oauth2::save_secret(&inst::oauth_secret_file(instance_name)?, &app_secret).await
    }
}
