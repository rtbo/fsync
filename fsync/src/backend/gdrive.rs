use std::str;

use async_stream::try_stream;
use camino::{Utf8Path, Utf8PathBuf};
use futures::Stream;

use crate::{cipher, http, oauth2, DirEntries, Entry, EntryType, PathId};

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

#[derive(Clone)]
pub struct GoogleDrive {
    auth: oauth2::Authenticator,
    client: hyper::Client<http::Connector>,
    base_url: &'static str,
    upload_base_url: &'static str,
    user_agent: String,
}

impl GoogleDrive {
    pub async fn new(oauth2_params: oauth2::Params<'_>) -> crate::Result<Self> {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_only()
            .enable_all_versions()
            .build();
        let client = hyper::Client::builder().build(connector);
        let auth = oauth2::installed_flow(oauth2_params, client.clone()).await?;
        let user_agent = format!("fsyncd/{}", env!("CARGO_PKG_VERSION"));
        Ok(Self {
            auth,
            client,
            base_url: "https://www.googleapis.com/drive/v3",
            upload_base_url: "https://www.googleapis.com/upload/drive/v3",
            user_agent,
        })
    }
}

impl DirEntries for GoogleDrive {
    fn dir_entries(
        &self,
        parent_path_id: Option<PathId>,
    ) -> impl Stream<Item = crate::Result<Entry>> + Send {
        let parent_id = parent_path_id.map(|di| di.id).unwrap_or("root");
        let base_dir = parent_path_id.map(|di| di.path);
        let q = format!("'{}' in parents", parent_id);
        let mut next_page_token: Option<String> = None;

        try_stream! {
            loop {
                let file_list = self.files_list(q.clone(), next_page_token).await?;
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

impl crate::ReadFile for GoogleDrive {
    async fn read_file<'a>(&self, path_id: PathId<'a>) -> crate::Result<impl tokio::io::AsyncRead> {
        Ok(self
            .files_get_media(path_id.id)
            .await?
            .expect("Could not find file"))
    }
}

impl crate::CreateFile for GoogleDrive {
    async fn create_file(&self, metadata: &Entry, data: impl tokio::io::AsyncRead) -> crate::Result<()> {
        unimplemented!()
        // debug_assert!(metadata.path().is_relative());
        // let path = self.root.join(metadata.path());
        // if path.is_dir() {
        //     return Err(crate::Error::Custom(format!(
        //         "{} is a directory",
        //         metadata.path()
        //     )));
        // }

        // tokio::pin!(data);

        // let mut f = tokio::fs::File::create(&path).await?;
        // tokio::io::copy(&mut data, &mut f).await?;

        // if let Some(mtime) = metadata.mtime() {
        //     let f = f.into_std().await;
        //     f.set_modified(mtime.into())?;
        // }
        // Ok(())
    }
}

impl crate::Storage for GoogleDrive {}

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
            crate::Error::Custom(format!("Expected to receive size from Google for {path}"))
        })? as _;
        EntryType::Regular { size, mtime }
    };

    Ok(Entry::new(id, path, typ))
}

mod api {
    use chrono::{DateTime, Utc};
    use http::StatusCode;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use tokio::io;

    use super::utils;

    #[derive(Default, Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct File {
        pub id: Option<String>,
        pub name: Option<String>,
        pub modified_time: Option<DateTime<Utc>>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            serialize_with = "size_to_str",
            deserialize_with = "size_from_str"
        )]
        pub size: Option<u64>,
        pub mime_type: Option<String>,
    }

    fn size_to_str<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = value.expect("size_to_str shouldn't be called for None");
        let value = value.to_string();
        serializer.serialize_str(&value)
    }

    fn size_from_str<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        use std::str::FromStr;

        let s = String::deserialize(deserializer)?;
        Ok(Some(u64::from_str(&s).map_err(serde::de::Error::custom)?))
    }

    #[derive(Default, Clone, Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FileList {
        pub files: Option<Vec<File>>,
        pub incomplete_search: Option<bool>,
        pub kind: Option<String>,
        pub next_page_token: Option<String>,
    }

    pub enum Scope {
        Full,
        MetadataReadOnly,
    }

    impl AsRef<str> for Scope {
        fn as_ref(&self) -> &str {
            match self {
                Scope::Full => "https://www.googleapis.com/auth/drive",
                Scope::MetadataReadOnly => {
                    "https://www.googleapis.com/auth/drive.metadata.readonly"
                }
            }
        }
    }

    #[derive(Debug, Copy, Clone)]
    pub enum UploadType {
        Simple,
        Multipart,
        Resumable,
    }

    impl AsRef<str> for UploadType {
        fn as_ref(&self) -> &str {
            match self {
                UploadType::Simple => "simple",
                UploadType::Multipart => "multipart",
                UploadType::Resumable => "resumable",
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct UploadParams {
        pub typ: UploadType,
        pub size: Option<u64>,
        pub mime_type: Option<String>,
    }

    const METADATA_FIELDS: &str = "id,name,size,modifiedTime,mimeType";

    impl super::GoogleDrive {
        pub async fn files_list(
            &self,
            q: String,
            page_token: Option<String>,
        ) -> crate::Result<FileList> {
            let mut query_params = vec![
                ("q", q),
                ("fields", format!("files({METADATA_FIELDS})")),
                ("alt", "json".into()),
            ];
            if let Some(page_token) = page_token {
                query_params.push(("pageToken", page_token));
            }

            let mut res = self
                .get_request_query(&[Scope::MetadataReadOnly], "/files", query_params, false)
                .await?;

            let body = utils::get_body_as_string(res.body_mut()).await;
            let file_list: FileList = serde_json::from_str(&body)?;

            Ok(file_list)
        }

        pub async fn files_get_media(
            &self,
            file_id: &str,
        ) -> crate::Result<Option<impl io::AsyncRead>> {
            let path = format!("/files/{file_id}");
            let query_params = &[("fields", METADATA_FIELDS), ("alt", "media")];
            let res = self
                .get_request_query(&[Scope::Full], &path, query_params, true)
                .await?;

            if res.status() == StatusCode::NOT_FOUND {
                Ok(None)
            } else {
                use futures::stream::{StreamExt, TryStreamExt};

                let body = res.into_body();
                let body = body.map(|res| {
                    res.map_err(|err| {
                        std::io::Error::new(std::io::ErrorKind::Other, err.to_string())
                    })
                });
                let read = body.into_async_read();

                Ok(Some(tokio_util::compat::FuturesAsyncReadCompatExt::compat(
                    read,
                )))
            }
        }

        pub async fn files_create<D>(&self, metadata: &File, data: D) -> crate::Result<()>
        where
            D: io::AsyncRead,
        {
            let upload_params = UploadParams {
                typ: UploadType::Resumable,
                size: metadata.size,
                mime_type: metadata.mime_type.clone(),
            };
            let upload_url = self
                .post_upload_request(&[Scope::Full], "/files", &upload_params, Some(metadata))
                .await?;
            unimplemented!("files_create")
        }
    }
}

mod utils {
    use std::borrow::Borrow;

    use http::{header, HeaderValue, Request, Response, StatusCode};
    use hyper::Body;
    use serde::Serialize;
    use url::Url;

    use super::api;
    use crate::oauth2::AccessToken;

    impl super::GoogleDrive {
        pub async fn fetch_token(&self, scopes: &[api::Scope]) -> crate::Result<AccessToken> {
            let token = self.auth.token(scopes).await?;

            if token.is_expired() {
                panic!("expired token");
            }

            Ok(token)
        }

        pub async fn get_request_query<Q, K, V>(
            &self,
            scopes: &[api::Scope],
            path: &str,
            query_params: Q,
            allow_404: bool,
        ) -> crate::Result<Response<Body>>
        where
            Q: IntoIterator,
            Q::Item: Borrow<(K, V)>,
            K: AsRef<str>,
            V: AsRef<str>,
        {
            let token = self.fetch_token(scopes).await?;
            let url = url_with_query(&self.base_url, path, query_params);

            let req = Request::builder()
                .uri(url.as_str())
                .header(header::USER_AGENT, &self.user_agent)
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", token.token().unwrap()),
                )
                .body(hyper::body::Body::empty())
                .expect("invalid request");

            let res = self.client.request(req).await?;

            if res.status().is_success() || (allow_404 && res.status() == StatusCode::NOT_FOUND) {
                Ok(res)
            } else {
                Err(crate::http::Error::Status(res).into())
            }
        }

        pub async fn post_upload_request<B>(
            &self,
            scopes: &[api::Scope],
            path: &str,
            params: &api::UploadParams,
            body: Option<&B>,
        ) -> crate::Result<String>
        where
            B: Serialize,
        {
            let token = self.fetch_token(scopes).await?;

            let query_params = &[("uploadType", params.typ)];
            let url = url_with_query(&self.upload_base_url, path, query_params);
            let mut req = Request::builder()
                .method("POST")
                .uri(url.as_str())
                .header(header::USER_AGENT, &self.user_agent)
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", token.token().unwrap()),
                );
            if let Some(mt) = &params.mime_type {
                req = req.header("X-Upload-Content-Type", mt);
            }
            if let Some(sz) = params.size {
                req = req.header("X-Upload-Content-Length", sz);
            }

            let body = body.map(serde_json::to_string).transpose()?;
            if let Some(body) = body.as_deref() {
                req = req
                    .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                    .header(header::CONTENT_LENGTH, body.len());
            }
            let body = body.unwrap_or_default();
            let req = req.body(hyper::body::Body::from(body)).unwrap();

            let mut res = self.client.request(req).await?;
            if res.status() != StatusCode::OK {
                return Err(crate::http::Error::Status(res).into());
            }
            println!("{}", get_body_as_string(res.body_mut()).await);
            let location = &res.headers()[header::LOCATION];
            Ok(location.to_str().unwrap().to_string())
        }
    }

    pub fn url_with_query<B, P, Q, K, V>(base_url: B, path: P, query_params: Q) -> Url
    where
        B: AsRef<str>,
        P: AsRef<str>,
        Q: IntoIterator,
        Q::Item: Borrow<(K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let base = format!("{}{}", base_url.as_ref(), path.as_ref());
        Url::parse_with_params(&base, query_params).unwrap()
    }

    pub async fn get_body_as_string(body: &mut Body) -> String {
        let buf = hyper::body::to_bytes(body).await.unwrap();
        let body_str = String::from_utf8_lossy(&buf);
        body_str.to_string()
    }
}
