use std::sync::Arc;

use futures::prelude::*;
use oauth2::{basic::BasicClient, HttpRequest, HttpResponse, TokenResponse};
pub use oauth2::{AccessToken, RefreshToken, Scope};
use tokio::sync::RwLock;

mod pkce;
mod server;
mod token_cache;

pub use self::token_cache::{CacheResult, TokenCache, TokenPersist, TokenStore};
use crate::PersistCache;

pub trait GetToken: Send + Sync + 'static {
    fn get_token(
        &self,
        scopes: Vec<Scope>,
    ) -> impl Future<Output = anyhow::Result<AccessToken>> + Send;
}

#[derive(Debug)]
struct Inner {
    cache: RwLock<TokenCache>,
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

        Ok(Self {
            inner: Arc::new(Inner {
                cache,
                http,
                oauth2,
            }),
        })
    }

    async fn refresh_token(
        &self,
        refresh_token: RefreshToken,
        scopes: Vec<Scope>,
    ) -> anyhow::Result<AccessToken> {
        let token_response = self
            .inner
            .oauth2
            .exchange_refresh_token(&refresh_token)
            .add_scopes(scopes.clone())
            .request_async(|req| async { self.http(req).await })
            .await?;

        let access = token_response.access_token().to_owned();

        let mut cache = self.inner.cache.write().await;
        cache.put(&token_response);

        Ok(access)
    }

    async fn http(&self, req: HttpRequest) -> reqwest::Result<HttpResponse> {
        let method = req.method.clone();
        let url = req.url.clone();

        let resp = self
            .inner
            .http
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

impl GetToken for Client {
    async fn get_token(&self, scopes: Vec<Scope>) -> anyhow::Result<AccessToken> {
        let cache = self.inner.cache.read().await.check(&scopes);
        match cache {
            CacheResult::Ok(access_token) => Ok(access_token),
            CacheResult::Expired(refresh_token, scopes) => {
                self.refresh_token(refresh_token, scopes).await
            }
            CacheResult::None => {
                let resp = self.fetch_token_pkce(scopes).await?;
                let mut cache = self.inner.cache.write().await;
                cache.put(&resp);
                Ok(resp.access_token().clone())
            }
        }
    }
}

impl PersistCache for Client {
    async fn persist_cache(&self) -> anyhow::Result<()> {
        self.inner.cache.read().await.persist_cache().await?;
        Ok(())
    }
}
