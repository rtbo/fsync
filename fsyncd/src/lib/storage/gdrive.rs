use std::{str, sync::Arc};

use async_stream::try_stream;
use fsync::path::PathBuf;
use futures::Stream;
use tokio::io;

use super::id::IdBuf;
use crate::oauth;
use crate::storage::id::Id;

#[derive(Clone)]
pub struct GoogleDrive {
    client: reqwest::Client,
    auth: Arc<oauth::Client>,
    base_url: &'static str,
    upload_base_url: &'static str,
    user_agent: String,
}

impl GoogleDrive {
    pub async fn new(oauth_params: fsync::oauth::Params<'_>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().build()?;
        let auth = oauth::Client::new(
            oauth_params.secret,
            oauth::TokenCache::MemoryAndDisk(oauth_params.token_cache_path.into()),
            None,
        )
        .await?;

        let user_agent = format!("fsyncd/{}", env!("CARGO_PKG_VERSION"));
        Ok(Self {
            auth: Arc::new(auth),
            client,
            base_url: "https://www.googleapis.com/drive/v3",
            upload_base_url: "https://www.googleapis.com/upload/drive/v3",
            user_agent,
        })
    }
}

impl super::id::DirEntries for GoogleDrive {
    fn dir_entries(
        &self,
        parent_id: Option<IdBuf>,
        parent_path: PathBuf,
    ) -> impl Stream<Item = anyhow::Result<(IdBuf, fsync::Metadata)>> + Send {
        debug_assert!(
            parent_id.is_some() || parent_path.is_root(),
            "none Id is for root only"
        );
        let search_id = parent_id.as_deref().unwrap_or_else(|| Id::new("root"));
        let q = format!("'{search_id}' in parents");
        let mut next_page_token = None;

        try_stream! {
            loop {
                let file_list = self.files_list(q.clone(), next_page_token).await?;
                next_page_token = file_list.next_page_token;
                if let Some(files) = file_list.files {
                    for f in files {
                        yield map_file(parent_path.clone(), f)?;
                    }
                }
                if next_page_token.is_none() {
                    break;
                }
            }
        }
    }
}

impl super::id::ReadFile for GoogleDrive {
    async fn read_file(&self, id: IdBuf) -> anyhow::Result<impl io::AsyncRead> {
        Ok(self
            .files_get_media(id.as_str())
            .await?
            .expect("Could not find file"))
    }
}

impl super::id::CreateFile for GoogleDrive {
    async fn create_file(
        &self,
        parent_id: Option<&Id>,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead,
    ) -> anyhow::Result<(IdBuf, fsync::Metadata)> {
        debug_assert!(metadata.path().is_absolute() && !metadata.path().is_root());
        let file = map_metadata(parent_id, None, metadata);
        let file = self
            .files_create(&file, metadata.size().unwrap(), data)
            .await?;
        map_file(metadata.path().parent().unwrap().to_owned(), file)
    }
}

impl super::id::Storage for GoogleDrive {
    async fn shutdown(&self) {
        let _ = self.auth.flush_cache().await; 
    }
}

const FOLDER_MIMETYPE: &str = "application/vnd.google-apps.folder";

fn map_file(parent_path: PathBuf, f: api::File) -> anyhow::Result<(IdBuf, fsync::Metadata)> {
    let id = f.id.unwrap_or_default();
    let path = parent_path.join(f.name.as_deref().unwrap());
    let metadata = if f.mime_type.as_deref() == Some(FOLDER_MIMETYPE) {
        fsync::Metadata::Directory { path }
    } else {
        let mtime = f.modified_time.ok_or_else(|| {
            anyhow::anyhow!("Expected to receive modifiedTime from Google for {path}")
        })?;
        let size = f
            .size
            .ok_or_else(|| anyhow::anyhow!("Expected to receive size from Google for {path}"))?
            as _;
        fsync::Metadata::Regular { path, size, mtime }
    };
    Ok((id, metadata))
}

fn map_metadata(parent_id: Option<&Id>, id: Option<&Id>, metadata: &fsync::Metadata) -> api::File {
    let mime_type = match metadata {
        fsync::Metadata::Directory { .. } => Some(FOLDER_MIMETYPE.to_string()),
        _ => None,
    };
    let parents = parent_id.map(|id| vec![id.to_owned()]);
    api::File {
        id: id.map(ToOwned::to_owned),
        name: Some(metadata.name().to_owned()),
        size: None,
        modified_time: metadata.mtime(),
        mime_type,
        parents,
    }
}

mod api {
    use chrono::{DateTime, Utc};
    use http::StatusCode;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use tokio::io;

    use crate::storage::id::IdBuf;

    #[derive(Default, Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct File {
        pub id: Option<IdBuf>,
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
        pub parents: Option<Vec<IdBuf>>,
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

    #[derive(Debug, Clone, Copy)]
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

    impl From<Scope> for oauth2::Scope {
        fn from(value: Scope) -> Self {
            oauth2::Scope::new(value.as_ref().to_string())
        }
    }

    #[derive(Debug, Copy, Clone)]
    pub enum UploadType {
        Simple,
        Multipart,
        Resumable,
    }

    impl UploadType {
        pub fn as_str(&self) -> &str {
            match self {
                UploadType::Simple => "simple",
                UploadType::Multipart => "multipart",
                UploadType::Resumable => "resumable",
            }
        }
    }

    impl AsRef<str> for UploadType {
        fn as_ref(&self) -> &str {
            self.as_str()
        }
    }

    #[derive(Debug, Clone)]
    pub struct UploadParams<'a> {
        pub typ: UploadType,
        pub size: Option<u64>,
        pub mime_type: Option<&'a str>,
        pub fields: &'a str,
    }

    impl<'a> UploadParams<'a> {
        pub fn query_params(&'a self) -> Vec<(&'static str, &'a str)> {
            vec![("uploadType", self.typ.as_str()), ("fields", self.fields)]
        }
    }

    const UPLOAD_CHUNK_SZ: u64 = 2 * 256 * 1024;
    const METADATA_FIELDS: &str = "id,name,size,modifiedTime,mimeType";

    impl super::GoogleDrive {
        pub async fn files_list(
            &self,
            q: String,
            page_token: Option<String>,
        ) -> anyhow::Result<FileList> {
            let mut query_params = vec![
                ("q", q),
                ("fields", format!("files({METADATA_FIELDS})")),
                ("alt", "json".into()),
            ];
            if let Some(page_token) = page_token {
                query_params.push(("pageToken", page_token));
            }

            let res = self
                .get_query(&[Scope::MetadataReadOnly], "/files", query_params, false)
                .await?;

            let file_list: FileList = res.json().await?;

            Ok(file_list)
        }

        pub async fn files_get_media(
            &self,
            file_id: &str,
        ) -> anyhow::Result<Option<impl io::AsyncRead>> {
            let path = format!("/files/{file_id}");
            let query_params = &[("fields", METADATA_FIELDS), ("alt", "media")];
            let res = self
                .get_query(&[Scope::Full], &path, query_params, true)
                .await?;

            if res.status() == StatusCode::NOT_FOUND {
                Ok(None)
            } else {
                use futures::stream::{StreamExt, TryStreamExt};

                let bytes = res.bytes_stream().map(|res| {
                    res.map_err(|err| {
                        std::io::Error::new(std::io::ErrorKind::Other, err.to_string())
                    })
                });
                let read = bytes.into_async_read();

                Ok(Some(tokio_util::compat::FuturesAsyncReadCompatExt::compat(
                    read,
                )))
            }
        }

        pub async fn files_create<D>(
            &self,
            file: &File,
            data_len: u64,
            data: D,
        ) -> anyhow::Result<File>
        where
            D: io::AsyncRead,
        {
            use io::AsyncReadExt;

            let scopes = &[Scope::Full];
            let upload_params = UploadParams {
                typ: UploadType::Resumable,
                size: file.size,
                mime_type: file.mime_type.as_deref(),
                fields: METADATA_FIELDS,
            };
            let upload_url = self
                .post_upload_request(scopes, "/files", &upload_params, Some(file))
                .await?;

            tokio::pin!(data);

            let mut sent = 0u64;
            let file: File = loop {
                let mut buf: Vec<u8> = Vec::with_capacity(UPLOAD_CHUNK_SZ as _);
                let sz = data
                    .as_mut()
                    .take(UPLOAD_CHUNK_SZ)
                    .read_to_end(&mut buf)
                    .await?;
                println!("uploading {sz} bytes");
                let res = self
                    .put_upload_range(scopes, upload_url.clone(), buf, sent, data_len)
                    .await?;
                sent += sz as u64;
                let status = res.status();
                if status.is_success() && sent == data_len {
                    break res.json().await?;
                } else if status.is_server_error() {
                    anyhow::bail!("Upload failed ({status}). No support yet to resume upload");
                } else if res.status().is_client_error() {
                    panic!(
                        "bad request ({status}): {}",
                        String::from_utf8_lossy(&res.bytes().await?)
                    );
                }
            };
            Ok(file)
        }
    }
}

mod utils {
    use std::borrow::Borrow;

    use oauth2::AccessToken;
    use reqwest::{header, Response, StatusCode, Url};
    use serde::Serialize;

    use super::api;

    impl super::GoogleDrive {
        pub async fn fetch_token(&self, scopes: &[api::Scope]) -> anyhow::Result<AccessToken> {
            let scopes = scopes.iter().map(|&s| s.into()).collect();
            Ok(self.auth.get_token(scopes).await?)
        }

        pub async fn get_query<Q, K, V>(
            &self,
            scopes: &[api::Scope],
            path: &str,
            query_params: Q,
            allow_404: bool,
        ) -> anyhow::Result<Response>
        where
            Q: IntoIterator,
            Q::Item: Borrow<(K, V)>,
            K: AsRef<str>,
            V: AsRef<str>,
        {
            let token = self.fetch_token(scopes).await?;
            let url = url_with_query(self.base_url, path, query_params);

            let res = self
                .client
                .get(url.clone())
                .header(header::USER_AGENT, &self.user_agent)
                .bearer_auth(token.secret())
                .send()
                .await?;

            if res.status().is_success() || (allow_404 && res.status() == StatusCode::NOT_FOUND) {
                Ok(res)
            } else {
                Err(anyhow::anyhow!("GET {url} returned {}", res.status()))
            }
        }

        pub async fn post_upload_request<'a, B>(
            &self,
            scopes: &[api::Scope],
            path: &str,
            params: &api::UploadParams<'_>,
            body: Option<&B>,
        ) -> anyhow::Result<Url>
        where
            B: Serialize,
        {
            let token = self.fetch_token(scopes).await?;

            let url = url_with_query(&self.upload_base_url, path, params.query_params());
            let mut req = self
                .client
                .post(url.clone())
                .bearer_auth(token.secret())
                .header(header::USER_AGENT, &self.user_agent);
            if let Some(mt) = params.mime_type {
                req = req.header("X-Upload-Content-Type", mt);
            }
            if let Some(sz) = params.size {
                req = req.header("X-Upload-Content-Length", sz);
            }
            if let Some(body) = body {
                req = req
                    .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                    //.header(header::CONTENT_LENGTH, body.len())
                    .json(body);
            }
            let res = req.send().await?;

            if res.status() != StatusCode::OK {
                anyhow::bail!("POST {url} returned {}", res.status());
            }
            let location = &res.headers()[header::LOCATION];
            Ok(Url::parse(location.to_str().unwrap())?)
        }

        pub async fn put_upload_range(
            &self,
            scopes: &[api::Scope],
            url: Url,
            data: Vec<u8>,
            range_start: u64,
            range_len: u64,
        ) -> anyhow::Result<Response> {
            let token = self.fetch_token(scopes).await?;

            let data_len = data.len() as u64;
            debug_assert!(range_len >= range_start + data_len);

            let mut req = self
                .client
                .put(url)
                .bearer_auth(token.secret())
                .header(header::USER_AGENT, &self.user_agent)
                .header(header::CONTENT_LENGTH, data_len);
            if range_start > 0 || data_len < range_len {
                req = req.header(
                    header::CONTENT_RANGE,
                    format!(
                        "bytes {range_start}-{}/{range_len}",
                        range_start + data_len - 1
                    ),
                );
            }
            Ok(req.body(data).send().await?)
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
}
