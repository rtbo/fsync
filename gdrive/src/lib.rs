use std::path;
use std::str;

// use std::task::{Context, Poll};
use async_stream::try_stream;
use camino::{Utf8Path, Utf8PathBuf};
use fsync::{Entry, EntryType, PathId, Result};
use futures::Stream;
use google_drive3::api;
use google_drive3::oauth2::authenticator::Authenticator;
use google_drive3::oauth2::ApplicationSecret;
use google_drive3::{oauth2, DriveHub};
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;

fn token_cache_path() -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let p = p.join("token_cache.json");
    p.into_os_string().into_string().unwrap()
}

pub type Connector = HttpsConnector<HttpConnector>;

const FOLDER_MIME_TYPE: &'static str = "application/vnd.google-apps.folder";

pub struct Storage {
    hub: DriveHub<Connector>,
}

impl Storage {
    pub async fn auth(secret: Option<ApplicationSecret>) -> Authenticator<Connector> {
        let secret = secret.unwrap_or_else(|| {
            let secret_json =
                unsafe { str::from_utf8_unchecked(include_bytes!("../client_secret.json")) };

            // Read application secret from a file. Sometimes it's easier to compile it directly into
            // the binary. The clientsecret file contains JSON like `{"installed":{"client_id": ... }}`
            oauth2::parse_application_secret(secret_json).expect("client_secret.json")
        });
        let token_cache = token_cache_path();
        oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .persist_tokens_to_disk(&token_cache)
        .build()
        .await
        .unwrap()
    }

    pub fn new(auth: Authenticator<Connector>) -> Self {
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
        Self { hub }
    }
}

impl fsync::Storage for Storage {
    fn entries(&self, dir_id: Option<PathId>) -> impl Stream<Item = Result<Entry>> + Send {
        let parent_id = dir_id.map(|di| di.id).unwrap_or("root");
        let base_dir = dir_id.map(|di| di.path);
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

fn map_file(base_dir: Option<&str>, f: api::File) -> Entry {
    let id = f.id.unwrap_or(String::new());
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
