use std::collections::HashMap;

use fsync::path::FsPath;
use fsyncd::{
    oauth2::GetToken,
    storage::{self, cache::CacheStorage, gdrive::GoogleDrive},
    PersistCache,
};
use futures::prelude::*;
use oauth2::{AccessToken, Scope};
use serde::{Deserialize, Serialize};
use tokio::{io, sync::OnceCell};

const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";

#[derive(Clone)]
struct Token(AccessToken);

impl GetToken for Token {
    async fn get_token(&self, scopes: Vec<Scope>) -> anyhow::Result<AccessToken> {
        assert_eq!(scopes, &[Scope::new(DRIVE_SCOPE.to_string())]);
        Ok(self.0.clone())
    }
}

impl PersistCache for Token {
    fn persist_cache(&self) -> impl Future<Output = anyhow::Result<()>> + Send {
        future::ready(Ok(()))
    }
}

static TOKEN: OnceCell<AccessToken> = OnceCell::const_new();

#[derive(Debug, Serialize, Deserialize)]
struct AccountKey {
    #[serde(rename = "type")]
    typ: String,
    project_id: String,
    private_key_id: String,
    private_key: String,
    client_email: String,
    client_id: String,
    auth_uri: String,
    token_uri: String,
    auth_provider_x509_cert_url: String,
    client_x509_cert_url: String,
    universe_domain: String,
}

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

async fn fetch_token_from_google() -> anyhow::Result<AccessToken> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let keypath = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("pkey.json");
    let key = tokio::fs::read_to_string(&keypath).await?;
    let account_key: AccountKey = serde_json::from_str(&key)?;

    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("JWT".to_string());
    header.kid = Some(account_key.private_key_id);

    let iat = jsonwebtoken::get_current_timestamp();
    let exp = iat + 1800;

    let claims = JwtClaims {
        iss: account_key.client_email,
        scope: DRIVE_SCOPE.to_string(),
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
    inner: CacheStorage<GoogleDrive<Token>>,
}

impl Stub {
    pub async fn new(path: &FsPath) -> anyhow::Result<Self> {
        let token = TOKEN.get_or_try_init(fetch_token_from_google).await?;
        let token = Token(token.clone());
        let drive = GoogleDrive::new(token, reqwest::Client::new(), None).await?;
        let cache = CacheStorage::new(drive, path.to_owned());
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
