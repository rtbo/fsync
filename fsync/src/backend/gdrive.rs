use std::str;

// use std::task::{Context, Poll};
use async_stream::try_stream;
use camino::{Utf8Path, Utf8PathBuf};
use futures::Stream;
use google_drive3::api;
use google_drive3::oauth2::ApplicationSecret;
use google_drive3::DriveHub;

use crate::cipher::decipher_text;
use crate::{oauth2, Entry, EntryType, PathId};

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
    pub fn get(self) -> crate::Result<ApplicationSecret> {
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
                let secret_json = decipher_text(CIPHERED_SECRET);
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
            } => Ok(ApplicationSecret {
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

pub struct Storage {
    hub: DriveHub<oauth2::Connector>,
}

impl Storage {
    pub async fn new(secret_path: &Utf8Path, token_cache_path: &Utf8Path) -> crate::Result<Self> {
        let app_secret = oauth2::load_secret(secret_path).await?;
        let auth = oauth2::installed_flow(app_secret, token_cache_path).await?;

        let hub = DriveHub::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .https_or_http()
                    .enable_http1()
                    .build(),
            ),
            auth,
        );
        Ok(Self { hub })
    }
}

impl crate::Storage for Storage {
    async fn entry<'a>(&self, path_id: PathId<'a>) -> crate::Result<Entry> {
        let (_resp, file) = self
            .hub
            .files()
            .get(path_id.id)
            .param("fields", METADATA_FIELDS)
            .doit()
            .await?;
        map_file(Some(path_id.path), file)
    }

    fn entries(
        &self,
        parent_path_id: Option<PathId>,
    ) -> impl Stream<Item = crate::Result<Entry>> + Send {
        let parent_id = parent_path_id.map(|di| di.id).unwrap_or("root");
        let base_dir = parent_path_id.map(|di| di.path);
        let q = format!("'{}' in parents", parent_id);
        let fields = format!("files({METADATA_FIELDS})");
        let mut next_page_token: Option<String> = None;
        try_stream! {
            loop {
                let mut query = self.hub.files().list().param("fields", &fields).q(&q);
                if let Some(page_token) = next_page_token {
                    query = query.page_token(&page_token);
                }
                let (_resp, file_list) = query.doit().await?;
                next_page_token = file_list.next_page_token;
                if let Some(files) = file_list.files {
                    for f in files {
                        yield map_file(base_dir, f)?;
                    }
                }
                if next_page_token.is_none() {
                    break;
                }
            }
        }
    }
}

const METADATA_FIELDS: &str = "id,name,size,modifiedTime,mimeType";
const FOLDER_MIMETYPE: &str = "application/vnd.google-apps.folder";

fn map_file(base_dir: Option<&Utf8Path>, f: api::File) -> crate::Result<Entry> {
    let id = f.id.unwrap_or_default();
    let path = match base_dir {
        Some(di) => Utf8Path::new(di).join(f.name.as_deref().unwrap()),
        None => Utf8PathBuf::from(f.name.as_deref().unwrap()),
    };
    let typ = if f.mime_type.as_deref() == Some(FOLDER_MIMETYPE) {
        EntryType::Directory
    } else {
        let mtime = f.modified_time.ok_or_else(|| {
            crate::Error::Custom(format!(
                "Expected to receive modifiedTime from Google for {path}"
            ))
        })?;
        let size = f.size.ok_or_else(|| {
            crate::Error::Custom(format!(
                "Expected to receive size from Google for {path}"
            ))
        })? as _;
        EntryType::Regular { size, mtime }
    };

    Ok(Entry::new(id, path, typ))
}
