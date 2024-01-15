use std::{str, sync::Arc};

use anyhow::Context;
use async_stream::try_stream;
use fsync::path::{PathBuf, Path, Component};
use futures::Stream;
use tokio::io;

use crate::{
    oauth::GetToken,
    storage::id::{Id, IdBuf},
    PersistCache,
};

#[derive(Clone)]
pub struct GoogleDrive<A> {
    client: reqwest::Client,
    auth: Arc<A>,
    base_url: &'static str,
    upload_base_url: &'static str,
    user_agent: String,

    root: IdBuf,
    user: api::User,
    quota: api::Quota,
}

impl<A> GoogleDrive<A>
where
    A: GetToken,
{
    pub async fn new(auth: A, client: reqwest::Client, root: Option<&Path>) -> anyhow::Result<Self> {
        let user_agent = format!("fsyncd/{}", env!("CARGO_PKG_VERSION"));
        let mut drive = Self {
            auth: Arc::new(auth),
            client,
            base_url: "https://www.googleapis.com/drive/v3",
            upload_base_url: "https://www.googleapis.com/upload/drive/v3",
            user_agent,
            root: IdBuf::from("root"),
            user: api::User::default(),
            quota: api::Quota::default(),
        };

        let about = drive.about_get().await?;
        drive.user = about.user;
        drive.quota = about.storage_quota;

        if let Some(root) = root {
            let root = drive.path_to_id(root).await?.with_context(|| format!("No such path: '{root}'"))?;
            drive.root = root;
        }

        log::info!(
            "Access granted to Drive of {}{}",
            drive.user.display_name,
            drive
                .user
                .email_address
                .as_ref()
                .map(|em| format!(" <{em}>"))
                .unwrap_or_default(),
        );
        if let (&Some(usage), &Some(limit)) = (&drive.quota.usage, &drive.quota.limit) {
            use byte_unit::{Byte, UnitType};
            let usage = Byte::from_i64(usage)
                .expect("positive")
                .get_appropriate_unit(UnitType::Binary);
            let limit = Byte::from_i64(limit)
                .expect("positive")
                .get_appropriate_unit(UnitType::Binary);
            log::info!("Usage {usage:#.2} / {limit:#.3}");
        }

        Ok(drive)
    }

    async fn path_to_id<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<Option<IdBuf>> {
        let path = path.as_ref().normalize()?;
        if path.is_relative() {
            anyhow::bail!("expected an absolute path, got '{path}'");
        }
        let mut cur_id = None;
        for comp in path.components() {
            match comp {
                Component::RootDir => cur_id = Some(IdBuf::from("root")),
                Component::Normal(name) => {
                    let q = format!("name = '{name}' and '{}' in parents", cur_id.unwrap());
                    let files = self.files_list(q, None).await?;
                    if files.files.is_none() {
                        return Ok(None);
                    }
                    let files = files.files.unwrap();
                    if files.len() != 1 {
                        return Ok(None);
                    }
                    let id = files.into_iter().next().unwrap().id;
                    if id.is_none() {
                        return Ok(None);
                    }
                    cur_id = id;
                }
                _ => unreachable!(),
            }
        }
        Ok(cur_id)
    }
}

impl<A> super::id::DirEntries for GoogleDrive<A>
where
    A: GetToken,
{
    fn dir_entries(
        &self,
        parent_id: Option<IdBuf>,
        parent_path: PathBuf,
    ) -> impl Stream<Item = anyhow::Result<(IdBuf, fsync::Metadata)>> + Send {
        debug_assert!(
            parent_id.is_some() || parent_path.is_root(),
            "none Id is for root only"
        );
        log::trace!("listing entries of {parent_path}");
        let search_id = parent_id.as_deref().unwrap_or(&self.root);
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

impl<A> super::id::ReadFile for GoogleDrive<A>
where
    A: GetToken,
{
    async fn read_file(&self, id: IdBuf) -> anyhow::Result<impl io::AsyncRead> {
        log::trace!("reading file {id}");
        Ok(self
            .files_get_media(id.as_str())
            .await?
            .expect("Could not find file"))
    }
}

impl<A> super::id::MkDir for GoogleDrive<A>
where
    A: GetToken,
{
    async fn mkdir(&self, parent_id: Option<&Id>, name: &str) -> anyhow::Result<IdBuf> {
        if let Some(parent_id) = parent_id {
            log::info!("creating folder {name} in folder {parent_id}");
        } else {
            log::info!("creating folder {name} in root folder");
        }
        let f = api::File {
            id: None,
            name: Some(name.to_string()),
            modified_time: None,
            size: None,
            mime_type: Some(FOLDER_MIMETYPE.to_string()),
            parents: parent_id.map(|id| vec![id.to_id_buf()]),
        };
        let res = self.files_create(&f).await?;
        Ok(res.id.context("No ID returned")?)
    }
}

impl<A> super::id::CreateFile for GoogleDrive<A>
where
    A: GetToken,
{
    async fn create_file(
        &self,
        parent_id: Option<&Id>,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead,
    ) -> anyhow::Result<(IdBuf, fsync::Metadata)> {
        debug_assert!(metadata.path().is_absolute() && !metadata.path().is_root());
        log::info!(
            "creating file {} ({} bytes)",
            metadata.path(),
            metadata.size().unwrap()
        );
        let file = map_metadata(parent_id, None, metadata);
        let file = self
            .files_create_upload(&file, metadata.size().unwrap(), data)
            .await?;
        map_file(metadata.path().parent().unwrap().to_owned(), file)
    }
}

impl<A> PersistCache for GoogleDrive<A>
where
    A: PersistCache + Send + Sync,
{
    async fn persist_cache(&self) -> anyhow::Result<()> {
        self.auth.persist_cache().await
    }
}

impl<A> super::id::Storage for GoogleDrive<A> where A: GetToken + PersistCache {}

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
    use serde::{Deserialize, Serialize};
    use tokio::io;

    use super::utils::{check_response, num_from_str, num_to_str};
    use crate::{oauth::GetToken, storage::id::IdBuf};

    #[derive(Default, Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct User {
        kind: String,
        pub display_name: String,
        pub me: bool,
        pub permission_id: String,
        pub email_address: Option<String>,
        pub photo_link: Option<String>,
    }

    #[derive(Default, Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Quota {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            serialize_with = "num_to_str",
            deserialize_with = "num_from_str"
        )]
        pub limit: Option<i64>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            serialize_with = "num_to_str",
            deserialize_with = "num_from_str"
        )]
        pub usage_in_drive: Option<i64>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            serialize_with = "num_to_str",
            deserialize_with = "num_from_str"
        )]
        pub usage_in_drive_trash: Option<i64>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            serialize_with = "num_to_str",
            deserialize_with = "num_from_str"
        )]
        pub usage: Option<i64>,
    }

    const ABOUT_FIELDS: &str = "kind,storageQuota,user";

    #[derive(Default, Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct About {
        kind: String,
        pub storage_quota: Quota,
        pub user: User,
    }

    const FILE_FIELDS: &str = "id,name,size,modifiedTime,mimeType";

    #[derive(Default, Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct File {
        pub id: Option<IdBuf>,
        pub name: Option<String>,
        pub modified_time: Option<DateTime<Utc>>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            serialize_with = "num_to_str",
            deserialize_with = "num_from_str"
        )]
        pub size: Option<i64>,
        pub mime_type: Option<String>,
        pub parents: Option<Vec<IdBuf>>,
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

    impl<A> super::GoogleDrive<A>
    where
        A: GetToken,
    {
        pub async fn about_get(&self) -> anyhow::Result<About> {
            let path = "/about";
            let query_params = vec![("fields", ABOUT_FIELDS)];

            let res = self
                .get_query(&[Scope::MetadataReadOnly], path, query_params)
                .await?;
            let res = check_response("GET", &path, res).await?;
            let about: About = res.json().await?;
            if about.kind != "drive#about" {
                anyhow::bail!("/about returned wrong kind!");
            }
            Ok(about)
        }

        pub async fn files_list(
            &self,
            q: String,
            page_token: Option<String>,
        ) -> anyhow::Result<FileList> {
            let path = "/files";

            let mut query_params = vec![
                ("q", q),
                ("fields", format!("files({FILE_FIELDS})")),
                ("alt", "json".into()),
            ];
            if let Some(page_token) = page_token {
                query_params.push(("pageToken", page_token));
            }

            let res = self
                .get_query(&[Scope::MetadataReadOnly], path, query_params)
                .await?;
            let res = check_response("GET", &path, res).await?;

            let file_list: FileList = res.json().await?;

            Ok(file_list)
        }

        pub async fn files_get_media(
            &self,
            file_id: &str,
        ) -> anyhow::Result<Option<impl io::AsyncRead>> {
            use futures::stream::{StreamExt, TryStreamExt};

            let path = format!("/files/{file_id}");
            let query_params = &[("fields", FILE_FIELDS), ("alt", "media")];

            let res = self.get_query(&[Scope::Full], &path, query_params).await?;
            if res.status() == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let res = check_response("GET", &path, res).await?;

            let bytes = res.bytes_stream().map(|res| {
                res.map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))
            });
            let read = bytes.into_async_read();

            Ok(Some(tokio_util::compat::FuturesAsyncReadCompatExt::compat(
                read,
            )))
        }

        pub async fn files_create(&self, file: &File) -> anyhow::Result<File> {
            let scopes = &[Scope::Full];
            let path = "/files";
            let query_params = &[("fields", FILE_FIELDS)];
            let res = self
                .post_json_query(scopes, path, query_params, file)
                .await?;
            let res = check_response("POST", path, res).await?;

            let file: File = res.json().await?;
            Ok(file)
        }

        pub async fn files_create_upload<D>(
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
                size: file.size.map(|sz| sz as _),
                mime_type: file.mime_type.as_deref(),
                fields: FILE_FIELDS,
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
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::api;
    use crate::oauth::GetToken;

    pub fn num_to_str<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = value.expect("size_to_str shouldn't be called for None");
        let value = value.to_string();
        serializer.serialize_str(&value)
    }

    pub fn num_from_str<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        use std::str::FromStr;

        let s = String::deserialize(deserializer)?;
        Ok(Some(i64::from_str(&s).map_err(serde::de::Error::custom)?))
    }

    pub async fn check_response(
        method: &str,
        path: &str,
        res: Response,
    ) -> anyhow::Result<Response> {
        if !res.status().is_success() {
            anyhow::bail!(
                "{method} {path} returned {}\n{}",
                res.status(),
                res.text().await?
            );
        }
        Ok(res)
    }

    impl<A> super::GoogleDrive<A>
    where
        A: GetToken,
    {
        pub async fn fetch_token(&self, scopes: &[api::Scope]) -> anyhow::Result<AccessToken> {
            let scopes = scopes.iter().map(|&s| s.into()).collect();
            Ok(self.auth.get_token(scopes).await?)
        }

        pub async fn get_query<Q, K, V>(
            &self,
            scopes: &[api::Scope],
            path: &str,
            query_params: Q,
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

            Ok(res)
        }

        pub async fn post_json_query<T, Q, K, V>(
            &self,
            scopes: &[api::Scope],
            path: &str,
            query_params: Q,
            body: &T,
        ) -> anyhow::Result<Response>
        where
            T: Serialize,
            Q: IntoIterator,
            Q::Item: Borrow<(K, V)>,
            K: AsRef<str>,
            V: AsRef<str>,
        {
            let token = self.fetch_token(scopes).await?;
            let url = url_with_query(self.base_url, path, query_params);
            let res = self
                .client
                .post(url)
                .bearer_auth(token.secret())
                .header(header::USER_AGENT, &self.user_agent)
                .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                .json(body)
                .send()
                .await?;
            Ok(res)
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
