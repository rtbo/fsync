use std::net::SocketAddr;

use anyhow::Context;
use chrono::{DateTime, Utc};
use oauth2::{
    basic::{BasicClient, BasicTokenResponse},
    AccessToken, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, HttpRequest,
    HttpResponse, PkceCodeChallenge, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::{io, net};

use crate::{
    http::server,
    path::{FsPath, FsPathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TokenCache {
    NoCache,
    Memory,
    MemoryAndDisk(FsPathBuf),
}

impl TokenCache {
    fn try_path(&self) -> Option<&FsPath> {
        match self {
            Self::MemoryAndDisk(path) => Some(path),
            _ => None,
        }
    }

    fn has_mem(&self) -> bool {
        match self {
            Self::NoCache => false,
            Self::Memory => true,
            Self::MemoryAndDisk(_) => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedToken {
    scopes: Vec<String>,
    scopes_hash: u64,
    access_token: AccessToken,
    refresh_token: Option<RefreshToken>,
    expiration: Option<DateTime<Utc>>,
}

#[derive(Debug)]
enum CacheResult {
    Ok(AccessToken),
    Expired(RefreshToken, Vec<Scope>),
    None,
}

#[derive(Debug)]
struct TokenStore {
    cache: TokenCache,
    mem: Vec<CachedToken>,
}

impl TokenStore {
    async fn new(cache: TokenCache) -> anyhow::Result<Self> {
        let mem = if let Some(path) = cache.try_path() {
            Self::read_from_disk(path).await?
        } else {
            None
        };
        let mem = mem.unwrap_or_default();

        Ok(Self { cache, mem })
    }

    /// Attempts to read the cache from disk
    /// Returns `Ok(None)` if the path doesn't exist.
    /// Returns `Err` if the deserialization failed.
    async fn read_from_disk(path: &FsPath) -> anyhow::Result<Option<Vec<CachedToken>>> {
        let json = tokio::fs::read_to_string(path).await;
        if json.is_err() {
            return Ok(None);
        }
        let json = json.unwrap();
        let value = serde_json::from_str(&json)?;
        Ok(Some(value))
    }

    async fn write_to_disk(&self, path: &FsPath) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&self.mem)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }

    async fn push(&mut self, tok: &BasicTokenResponse) {
        if !self.cache.has_mem() {
            return;
        }

        let scopes = {
            let mut scopes: Vec<String> = tok
                .scopes()
                .map(|vec| vec.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default();
            scopes.sort_unstable();
            scopes
        };

        let scopes_hash = {
            use std::hash::{self, Hash, Hasher};
            let mut state = hash::DefaultHasher::new();
            scopes.hash(&mut state);
            state.finish()
        };
        let expiration = tok.expires_in().map(|exp| Utc::now() + exp);
        let token = CachedToken {
            scopes,
            scopes_hash,
            access_token: tok.access_token().clone(),
            refresh_token: tok.refresh_token().cloned(),
            expiration,
        };
        self.emplace_token(token);
        if let Some(path) = self.cache.try_path() {
            let _ = self.write_to_disk(path).await;
        }
    }

    fn emplace_token(&mut self, token: CachedToken) {
        for ct in self.mem.iter_mut() {
            if ct.scopes_hash == token.scopes_hash {
                *ct = token;
                return;
            }
        }
        self.mem.push(token);
    }

    fn pull(&self, scopes: &[Scope]) -> CacheResult {
        if !self.cache.has_mem() {
            return CacheResult::None;
        }
        for ct in self.mem.iter() {
            if !scopes.iter().all(|s| ct.scopes.contains(s)) {
                continue;
            }
            // ct contains all scopes, let's check expiration
            // Note: Typically only a handful of scopes are used with few combinations.
            // Therefore, to keep things simpler, we stop at the first hit that meet all
            // required scopes.
            if let Some(expiration) = ct.expiration {
                if expiration < Utc::now() {
                    if let Some(refresh_token) = &ct.refresh_token {
                        let scopes = ct.scopes.iter().map(|s| Scope::new(s.clone())).collect();
                        return CacheResult::Expired(refresh_token.clone(), scopes);
                    } else {
                        return CacheResult::None;
                    }
                }
            }
            return CacheResult::Ok(ct.access_token.clone());
        }
        CacheResult::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub auth_url: AuthUrl,
    pub token_url: TokenUrl,
}

#[derive(Debug)]
pub struct Client {
    token_store: RwLock<TokenStore>,
    http_client: reqwest::Client,
    client: BasicClient,
}

impl Client {
    pub async fn new(
        secret: Secret,
        token_cache: TokenCache,
        http_client: Option<reqwest::Client>,
    ) -> anyhow::Result<Self> {
        let token_store = TokenStore::new(token_cache).await?;
        let client = BasicClient::new(
            secret.client_id,
            Some(secret.client_secret),
            secret.auth_url,
            Some(secret.token_url),
        );
        let http_client = http_client.unwrap_or_else(|| reqwest::Client::new());

        Ok(Self {
            token_store: RwLock::new(token_store),
            http_client,
            client,
        })
    }

    pub async fn get_token(&self, scopes: Vec<Scope>) -> anyhow::Result<AccessToken> {
        let cache = self.token_store.read().await.pull(&scopes);
        match cache {
            CacheResult::Ok(access_token) => Ok(access_token),
            CacheResult::Expired(refresh_token, scopes) => {
                self.refresh_token(refresh_token, scopes).await
            }
            CacheResult::None => self.fetch_token(scopes).await,
        }
    }

    async fn refresh_token(
        &self,
        refresh_token: RefreshToken,
        scopes: Vec<Scope>,
    ) -> anyhow::Result<AccessToken> {
        let token_response = self
            .client
            .exchange_refresh_token(&refresh_token)
            .add_scopes(scopes.clone())
            .request_async(|req| async { self.http_client(req).await })
            .await?;

        let access = token_response.access_token().to_owned();

        let mut store = self.token_store.write().await;
        store.push(&token_response).await;

        Ok(access)
    }

    async fn fetch_token(&self, scopes: Vec<Scope>) -> anyhow::Result<AccessToken> {
        let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
        let listener = net::TcpListener::bind(&addr).await?;
        let redirect_addr = listener.local_addr()?;
        println!("bound to {redirect_addr}");
        let redirect_url = RedirectUrl::new(format!("http://{redirect_addr}"))?;
        let redirect_url = std::borrow::Cow::Borrowed(&redirect_url);

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let (auth_url, csrf_state) = self
            .client
            .authorize_url(CsrfToken::new_random)
            .set_redirect_uri(redirect_url.clone())
            .add_scopes(scopes)
            .set_pkce_challenge(pkce_challenge)
            .url();

        println!("auth url {auth_url}");
        println!("csrf state {}", csrf_state.secret());

        tokio::task::spawn_blocking(move || webbrowser::open(auth_url.as_str()));

        println!("now accepting");
        let (socket, _addr) = listener.accept().await?;
        println!("incoming from {_addr:#?}");
        let (reader, writer) = io::split(socket);
        let reader = io::BufReader::new(reader);
        let writer = io::BufWriter::new(writer);
        let req = server::parse_request(reader).await?;
        println!("got request {req:#?}");
        let query = crate::http::QueryMap::parse(req.uri().query())?;

        let code = query
            .get("code")
            .map(str::to_string)
            .map(AuthorizationCode::new)
            .context("Getting OAuth2 code")?;
        let state = query
            .get("state")
            .map(str::to_string)
            .map(CsrfToken::new)
            .context("Getting OAuth2 state")?;

        println!("got code {code:#?}");
        println!("got state {}", state.secret());

        if state.secret() != csrf_state.secret() {
            let res = server::Response::builder()
                .status(401)
                .body("Could not verify the CSRF token :-(".as_bytes());
            res.write(writer).await?;
            anyhow::bail!("Could not verify the CSRF token");
        }

        println!("exchanging code");
        let token_response = self
            .client
            .exchange_code(code)
            .set_pkce_verifier(pkce_verifier)
            .set_redirect_uri(redirect_url)
            .request_async(|req| async { self.http_client(req).await })
            .await?;

        let res = server::Response::builder()
            .status(200)
            .body("All good, you can close this window ;-)".as_bytes());
        res.write(writer).await?;

        let access = token_response.access_token().to_owned();

        let mut store = self.token_store.write().await;
        store.push(&token_response).await;

        Ok(access)
    }

    async fn http_client(&self, req: HttpRequest) -> reqwest::Result<HttpResponse> {
        let method = req.method.clone();
        let url = req.url.clone();

        let resp = self
            .http_client
            .request(req.method, req.url)
            .headers(req.headers)
            .body(req.body)
            .send()
            .await?;

        let status_code = resp.status();
        let headers = resp.headers().to_owned();
        let body = resp.bytes().await?.to_vec();

        if !status_code.is_success() {
            println!("{} {} received error {status_code}", method, url);
            if let Ok(body) = std::str::from_utf8(&body) {
                println!("{body}");
            }
        }

        Ok(HttpResponse {
            status_code,
            headers,
            body: body.into(),
        })
    }
}
