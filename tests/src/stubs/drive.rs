use std::{collections::HashMap, sync::Arc};

use fsync::path::{FsPath, Path};
use fsyncd::{
    oauth2::{GetToken, TokenMap},
    storage::{
        self,
        cache::{CachePersist, CacheStorage},
        drive::{GoogleDrive, RootSpec},
    },
    PersistCache,
};
use futures::prelude::*;
use oauth2::{AccessToken, Scope};
use serde::{Deserialize, Serialize};
use tokio::{
    io,
    sync::{OnceCell, RwLock},
};

use crate::{config, utils};

#[derive(Clone, Debug)]
struct TokenStore {
    map: Arc<RwLock<TokenMap<AccessToken>>>,
}

impl TokenStore {
    fn new() -> Self {
        Self {
            map: Arc::new(RwLock::new(TokenMap::new())),
        }
    }
}

impl GetToken for TokenStore {
    async fn get_token(&self, scopes: Vec<Scope>) -> anyhow::Result<AccessToken> {
        let token = self
            .map
            .read()
            .await
            .get(&scopes)
            .next()
            .map(|(tok, ..)| tok.clone());

        if let Some(token) = token {
            Ok(token)
        } else {
            let token = fetch_token_from_google(&scopes).await?;
            self.map.write().await.insert(scopes, token.clone());
            Ok(token)
        }
    }
}

impl PersistCache for TokenStore {}

static TOKEN: OnceCell<TokenStore> = OnceCell::const_new();

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    iss: String,
    scope: String,
    aud: String,
    iat: u64,
    exp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct OAuthResp {
    access_token: String,
    expires_in: u64,
    token_type: String,
}

async fn fetch_token_from_google(scopes: &[Scope]) -> anyhow::Result<AccessToken> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    assert_eq!(scopes.len(), 1, "only single scope query are supported");
    let scope = &scopes[0];

    let account_key = config::load_account_key().await?;

    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("JWT".to_string());
    header.kid = Some(account_key.private_key_id);

    let iat = jsonwebtoken::get_current_timestamp();
    let exp = iat + 1800;

    let claims = JwtClaims {
        iss: account_key.client_email,
        scope: scope.to_string(),
        aud: account_key.token_uri.clone(),
        iat,
        exp,
    };

    let key = EncodingKey::from_rsa_pem(account_key.private_key.as_bytes())?;

    let jwt = encode(&header, &claims, &key)?;

    let mut params = HashMap::new();
    params.insert("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer");
    params.insert("assertion", jwt.as_str());

    let client = reqwest::Client::new();
    let resp = client
        .post(account_key.token_uri.as_str())
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .form(&params)
        .send()
        .await?;

    assert!(resp.status().is_success());
    let resp: OAuthResp = resp.json().await?;

    Ok(AccessToken::new(resp.access_token))
}

#[derive(Clone)]
pub struct Stub {
    inner: CacheStorage<GoogleDrive<TokenStore>>,
}

impl Stub {
    pub async fn new(source: &FsPath) -> anyhow::Result<Self> {
        let token = TOKEN.get_or_init(|| future::ready(TokenStore::new())).await;
        let root_id = config::load_drive_root_id().await?;
        let root = RootSpec::SharedId(&root_id);
        let drive = GoogleDrive::new(token.clone(), reqwest::Client::new(), root).await?;
        drive.delete_folder_content(None, Path::root()).await?;

        let cache = CacheStorage::new(drive, CachePersist::Memory).await?;

        utils::copy_dir_all_to_storage(&cache, source, Path::root()).await?;

        Ok(Self { inner: cache })
    }
}

impl storage::DirEntries for Stub {
    fn dir_entries(
        &self,
        parent_path: fsync::path::PathBuf,
    ) -> impl Stream<Item = anyhow::Result<fsync::Metadata>> + Send {
        self.inner.dir_entries(parent_path)
    }
}

impl storage::ReadFile for Stub {
    fn read_file(
        &self,
        path: fsync::path::PathBuf,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead + Send>> + Send {
        self.inner.read_file(path)
    }
}

impl storage::MkDir for Stub {
    fn mkdir(
        &self,
        path: &fsync::path::Path,
        parents: bool,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        self.inner.mkdir(path, parents)
    }
}

impl storage::CreateFile for Stub {
    fn create_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<fsync::Metadata>> + Send {
        self.inner.create_file(metadata, data)
    }
}

impl fsyncd::Shutdown for Stub {}

impl storage::Storage for Stub {}
