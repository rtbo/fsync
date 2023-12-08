use std::future::Future;
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
use tokio::sync::mpsc::Sender;

fn token_cache_path() -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let p = p.join("token_cache.json");
    p.into_os_string().into_string().unwrap()
}

pub async fn list_my_files() {
    let secret_json = unsafe { str::from_utf8_unchecked(include_bytes!("../client_secret.json")) };

    // Read application secret from a file. Sometimes it's easier to compile it directly into
    // the binary. The clientsecret file contains JSON like `{"installed":{"client_id": ... }}`
    let app_secret = oauth2::parse_application_secret(secret_json).expect("client_secret.json");

    // Create an authenticator that uses an InstalledFlow to authenticate. The
    // authentication tokens are persisted to a file named token_cache.json. The
    // authenticator takes care of caching tokens to disk and refreshing tokens once
    // they've expired.
    let token_cache = token_cache_path();
    let auth = oauth2::InstalledFlowAuthenticator::builder(
        app_secret,
        oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .persist_tokens_to_disk(&token_cache)
    .build()
    .await
    .unwrap();

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

    let mut next_page_token: Option<String> = None;

    loop {
        // You can configure optional parameters by calling the respective setters at will, and
        // execute the final call using `doit()`.
        // Values shown here are possibly random and not representative !
        let mut query = hub
            .files()
            .list()
            // .team_drive_id("et")
            // .supports_team_drives(true)
            // .supports_all_drives(false)
            // .spaces("amet.")
            // .q("takimata")
            // .page_token("amet.")
            // .page_size(-20)
            // .order_by("ipsum")
            // .include_team_drive_items(true)
            // .include_permissions_for_view("Lorem")
            // .include_labels("gubergren")
            // .include_items_from_all_drives(false)
            // .drive_id("dolor")
            // .corpus("ea")
            // .corpora("ipsum")
            // .q("'root' in parents and mimeType='application/vnd.google-apps.folder'")
        ;
        if let Some(page_token) = next_page_token.as_deref() {
            query = query.page_token(page_token);
        }

        let result = query.doit().await.unwrap();

        next_page_token = result.1.next_page_token;

        if let Some(files) = result.1.files {
            for f in files {
                println!("{}", f.name.as_deref().unwrap_or("<no name>"));
                println!("   {:?}", f.parents);
            }
        }

        if next_page_token.is_none() {
            break;
        }
    }
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
        Self {
            hub,
        }
    }
}

fn map_file<'a>(base_dir: Option<&str>, f: api::File) -> Entry {
    //println!("{:#?}", &f);
    let id = f.id.unwrap_or(String::new());
    let path = match base_dir {
        Some(di) => Utf8Path::new(di).join(f.name.as_deref().unwrap()),
        None => Utf8PathBuf::from(f.name.as_deref().unwrap()),
    };
    //println!("  {path}: {:?}", f.mime_type);
    let typ = if f.mime_type.as_deref() == Some(FOLDER_MIME_TYPE) {
        EntryType::Directory
    } else {
        let mtime = f.modified_time;
        let size = f.size.unwrap_or(0) as _;
        EntryType::Regular { size, mtime }
    };

    Entry::new(id, path, typ)
}

impl fsync::Storage for Storage {
    async fn entries<'a>(&self, dir_id: Option<PathId<'a>>, sender: Sender<Entry>) -> Result<()> {
        let parent_id = dir_id.map(|di| di.id).unwrap_or("root");
        let base_dir = dir_id.map(|di| di.path);
        let q = format!("'{}' in parents", parent_id);
        let mut next_page_token: Option<String> = None;
        loop {
            let mut query = self.hub.files().list().q(&q);
            if let Some(page_token) = next_page_token {
                query = query.page_token(&page_token);
            }

            let (_resp, file_list) = query.doit().await?;
            next_page_token = file_list.next_page_token;
            if let Some(files) = file_list.files {
                for f in files {
                    sender.send(map_file(base_dir, f)).await.unwrap();
                }
            }
            if next_page_token.is_none() {
                break Ok(());
            }
        }
    }

    fn entries2(
        &self,
        dir_id: Option<PathId>,
    ) -> impl Future<Output = impl Stream<Item = Result<Entry>> + Send> + Send {

        let parent_id = dir_id.map(|di| di.id).unwrap_or("root");
        let base_dir = dir_id.map(|di| di.path);
        let q = format!("'{}' in parents", parent_id);
        let mut next_page_token: Option<String> = None;
        async move {
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
}

// struct EntryStream {
//     hub: DriveHub<Connector>,
//     q: String,
//     base_dir: Option<String>,
//     init_done: bool,
//     files: Vec<api::File>,
//     files_index: usize,
//     next_page_token: Option<String>,
// }

// impl Stream for EntryStream {
//     type Item = Entry;

//     fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         if self.files.len() > self.files_index {
//             let index = self.files_index;
//             self.files_index += 1;
//             return Poll::Ready(Some(map_file(self.base_dir.as_deref(), self.files[index])));
//         }
//         if self.init_done && self.files.is_empty() && self.next_page_token.is_none() {
//             return Poll::Ready(None);
//         }
//     }
// }
