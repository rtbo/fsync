use std::sync::Arc;

use fsync::Progress;
use futures::prelude::*;
use oauth2::{basic::BasicClient, HttpRequest, HttpResponse, TokenResponse};
pub use oauth2::{AccessToken, RefreshToken, Scope};
use tokio::sync::{Mutex, RwLock};

mod pkce;
mod server;
mod token_cache;

pub use self::token_cache::{CacheResult, TokenCache, TokenMap, TokenPersist};
use crate::{error, PersistCache, SharedProgress};

pub trait GetToken: Send + Sync + 'static {
    fn get_token(
        &self,
        scopes: Vec<Scope>,
        progress: Option<&SharedProgress>,
    ) -> impl Future<Output = fsync::Result<AccessToken>> + Send;
}

#[derive(Debug)]
struct Inner {
    cache: RwLock<TokenCache>,
    lock: Mutex<()>,
    http: reqwest::Client,
    oauth2: BasicClient,
}

#[derive(Clone, Debug)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    pub async fn new(
        secret: fsync::oauth2::Secret,
        persist: TokenPersist,
        http: Option<reqwest::Client>,
    ) -> anyhow::Result<Self> {
        let cache = TokenCache::new(persist).await?;
        let cache = RwLock::new(cache);
        let oauth2 = BasicClient::new(
            secret.client_id,
            Some(secret.client_secret),
            secret.auth_url,
            Some(secret.token_url),
        );
        let http = http.unwrap_or_else(|| reqwest::Client::new());
        let lock = Mutex::new(());

        Ok(Self {
            inner: Arc::new(Inner {
                cache,
                lock,
                http,
                oauth2,
            }),
        })
    }

    async fn refresh_token(
        &self,
        refresh_token: RefreshToken,
        scopes: Vec<Scope>,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<AccessToken> {
        log::info!("Refreshing token for scopes {:?}", scopes);

        if let Some(progress) = progress {
            progress.set(Progress::OAuth2Refresh);
        }

        let token_response = self
            .inner
            .oauth2
            .exchange_refresh_token(&refresh_token)
            .add_scopes(scopes)
            .request_async(|req| async { self.http(req).await })
            .await
            .map_err(error::auth)?;

        let access = token_response.access_token().to_owned();

        let mut cache = self.inner.cache.write().await;
        cache.put(&token_response);

        Ok(access)
    }

    async fn pkce_and_cache(
        &self,
        scopes: Vec<Scope>,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<AccessToken> {
        let resp = self.fetch_token_pkce(scopes, progress).await?;
        let mut cache = self.inner.cache.write().await;
        cache.put(&resp);
        Ok(resp.access_token().clone())
    }

    async fn http(&self, req: HttpRequest) -> reqwest::Result<HttpResponse> {
        let method = req.method.clone();
        let url = req.url.clone();

        log::trace!("OAUTH2 HTTP request: {method} {url}");

        let resp = self
            .inner
            .http
            .request(req.method, req.url)
            .headers(req.headers)
            .body(req.body)
            .send()
            .await?;

        let status_code = resp.status();

        log::trace!("OAUTH2 HTTP response: {status_code}");

        let headers = resp.headers().to_owned();
        let body = resp.bytes().await?.to_vec();

        if !status_code.is_success() {
            log::warn!("{method} {url} received error {status_code}");
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

impl GetToken for Client {
    async fn get_token(
        &self,
        scopes: Vec<Scope>,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<AccessToken> {
        log::trace!("getting token for scopes {scopes:?}");
        let _lock = self.inner.lock.lock().await;
        let cache = self.inner.cache.read().await.check(&scopes);
        match cache {
            CacheResult::Ok(access_token) => Ok(access_token),
            CacheResult::Expired(refresh_token, scopes) => {
                self.refresh_token(refresh_token, scopes.clone(), progress)
                    .or_else(|_err| self.pkce_and_cache(scopes, progress))
                    .await
            }
            CacheResult::None => self.pkce_and_cache(scopes, progress).await,
        }
    }
}

impl PersistCache for Client {
    async fn persist_cache(&self) -> anyhow::Result<()> {
        self.inner.cache.read().await.persist_cache().await?;
        Ok(())
    }
}
