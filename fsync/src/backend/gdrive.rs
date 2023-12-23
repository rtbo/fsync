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

impl AppSecretOpts {
    pub fn get(self) -> crate::Result<ApplicationSecret> {
        match self {
            AppSecretOpts::Fsync => {
                const CIPHERED_SECRET: &str = concat!(
                    "GB3fSrPXMUAIOLstLrJ8AlA3MyM6KULxtenYrt76NRXWPCn+VZiMZ+y5rEKCKaH/4i26lGa6azK44",
                    "zaTdPGrqzHo/D78cKQaQ3AeS9PRtF8UZK7JytDMs9fp5i00Ou/UW3iyLObnPlOKdh16dlUui7es7a",
                    "kr+HoMIdLjbh0yOH2FcEQhULkXFg4Dhj62CxPasI9JzYKkjMHuvQlyQA2NMfpGyGGGv42xR/Rdsxf",
                    "avIui8sGKjZ0lbMVg114pceT7YTNjSGHuNDCfbA9mdC9VnuG/dzqCot9pj+u7p1C0BJ2ks6cQ19rA",
                    "Z79zz/GH4kngQQJPXxvz0JF8b2xHhVAErlJX2+aomhbLRupsa9VVJHaEjAnPCdgOhYBY+NDOho+71",
                    "9JdTNW8Z1PGv8w0jIeKlyyBKdoGimQUybqoG12rpZgnkN+rWYEdkv9CBACIIO2ukrDlyCEjspj7yA",
                    "yfcIeUBWsi5M4JBUyI0G6gZQ9Pxs0irsDX3weBjI/0sqgVsDGhXn5V+N3eiO9JL7G1Xk8MQQB/Iqx",
                    "gRFGO/jQ6kRmRzwkfW2FPiEJLJDRXu9m+q2D7DNoT7Kw++v0OGVHIxy0UVeyQRe1dNbSq9JMiZ3Vx",
                    "VYxlRVhRH8+Vv15boyRT0/9WmlELhI9vCjpqmoAiLbxFHYfS91PXtetZx+LpSmMcz5wkSfJPdkAB3L0"
                );
                let secret_json = decipher_text(CIPHERED_SECRET);
                Ok(yup_oauth2::parse_application_secret(secret_json)?)
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
    _cache_dir: oauth2::CacheDir,
}

impl Storage {
    pub async fn new(cache_dir: oauth2::CacheDir) -> crate::Result<Self> {
        let app_secret = cache_dir.load_secret().await?;
        let auth = cache_dir.oauth2_installed_flow(app_secret).await?;

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
        Ok(Self {
            hub,
            _cache_dir: cache_dir,
        })
    }
}

impl crate::Storage for Storage {
    async fn entry<'a>(&self, path_id: PathId<'a>) -> crate::Result<Entry> {
        let (_resp, file) = self.hub.files().get(path_id.id).doit().await?;
        Ok(map_file(Some(path_id.path), file))
    }

    fn entries(
        &self,
        parent_path_id: Option<PathId>,
    ) -> impl Stream<Item = crate::Result<Entry>> + Send {
        let parent_id = parent_path_id.map(|di| di.id).unwrap_or("root");
        let base_dir = parent_path_id.map(|di| di.path);
        let q = format!("'{}' in parents", parent_id);
        let mut next_page_token: Option<String> = None;
        try_stream! {
            loop {
                let mut query = self.hub.files().list().q(&q);
                if let Some(page_token) = next_page_token {
                    query = query.page_token(&page_token);
                }
                let (_resp, file_list) = query.doit().await?;
                next_page_token = file_list.next_page_token;
                if let Some(files) = file_list.files {
                    for f in files {
                        yield map_file(base_dir, f);
                    }
                }
                if next_page_token.is_none() {
                    break;
                }
            }
        }
    }
}

const FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";

fn map_file(base_dir: Option<&Utf8Path>, f: api::File) -> Entry {
    let id = f.id.unwrap_or_default();
    let path = match base_dir {
        Some(di) => Utf8Path::new(di).join(f.name.as_deref().unwrap()),
        None => Utf8PathBuf::from(f.name.as_deref().unwrap()),
    };
    let typ = if f.mime_type.as_deref() == Some(FOLDER_MIME_TYPE) {
        EntryType::Directory
    } else {
        let mtime = f.modified_time;
        let size = f.size.unwrap_or(0) as _;
        EntryType::Regular { size, mtime }
    };

    Entry::new(id, path, typ)
}
