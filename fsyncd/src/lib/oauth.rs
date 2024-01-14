use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use chrono::{DateTime, Utc};
use fsync::path::{FsPath, FsPathBuf};
use futures::Future;
use oauth2::{
    basic::{BasicClient, BasicTokenResponse},
    AccessToken, AuthorizationCode, CsrfToken, HttpRequest, HttpResponse, PkceCodeChallenge,
    RedirectUrl, RefreshToken, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::{io, net};

use crate::{uri, PersistCache};

pub trait GetToken: Clone + Send + Sync + 'static {
    fn get_token(
        &self,
        scopes: Vec<Scope>,
    ) -> impl Future<Output = anyhow::Result<AccessToken>> + Send;
}

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
        log::info!("reading cached tokens from {path}");
        let json = tokio::fs::read_to_string(path).await;
        if json.is_err() {
            return Ok(None);
        }
        let json = json.unwrap();
        let value = serde_json::from_str(&json)?;
        Ok(Some(value))
    }

    async fn write_to_disk(&self, path: &FsPath) -> anyhow::Result<()> {
        log::info!("caching tokens to {path}");
        let json = serde_json::to_string_pretty(&self.mem)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        if let Some(path) = self.cache.try_path() {
            self.write_to_disk(path).await?;
        }
        Ok(())
    }

    /// Pushes a response in the cache for later retrieval.
    ///
    /// `push` will not attempt to write to disk.
    /// Loading/saving to disk is only done at start-up and shutdown.
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

        log::trace!(target:"oauth::TokenStore", "pushing token for scopes {scopes:#?}");

        let scopes_hash = {
            use std::hash::{Hash, Hasher};
            let mut state = std::collections::hash_map::DefaultHasher::new();
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

    fn dopull(&self, scopes: &[Scope]) -> CacheResult {
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

    fn pull(&self, scopes: &[Scope]) -> CacheResult {
        log::trace!(target: "oauth::TokenStore", "pulling token for scopes {scopes:#?}");
        let res = self.dopull(scopes);
        match &res {
            CacheResult::Ok(..) => log::trace!(target: "oauth::TokenStore", "Ok"),
            CacheResult::Expired(..) => log::trace!(target: "oauth::TokenStore", "Expired"),
            CacheResult::None => log::trace!(target: "oauth::TokenStore", "None"),
        }
        res
    }
}

#[derive(Debug)]
struct Inner {
    token_store: RwLock<TokenStore>,
    http_client: reqwest::Client,
    client: BasicClient,
}

#[derive(Clone, Debug)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    pub async fn new(
        secret: fsync::oauth::Secret,
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
            inner: Arc::new(Inner {
                token_store: RwLock::new(token_store),
                http_client,
                client,
            }),
        })
    }

    async fn refresh_token(
        &self,
        refresh_token: RefreshToken,
        scopes: Vec<Scope>,
    ) -> anyhow::Result<AccessToken> {
        let token_response = self.inner
            .client
            .exchange_refresh_token(&refresh_token)
            .add_scopes(scopes.clone())
            .request_async(|req| async { self.http_client(req).await })
            .await?;

        let access = token_response.access_token().to_owned();

        let mut store = self.inner.token_store.write().await;
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

        let (auth_url, csrf_state) = self.inner
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
        let query = uri::QueryMap::parse(req.uri().query())?;

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
            let resp = http::Response::builder()
                .status(401)
                .header("Date", Utc::now().to_rfc2822())
                .header("Server", "fsyncd")
                .header("Connection", "close")
                .body("Could not verify the CSRF token :-(")?;
            server::write_response(resp, writer).await?;
            anyhow::bail!("Could not verify the CSRF token");
        }

        println!("exchanging code");
        let token_response = self.inner
            .client
            .exchange_code(code)
            .set_pkce_verifier(pkce_verifier)
            .set_redirect_uri(redirect_url)
            .request_async(|req| async { self.http_client(req).await })
            .await?;

        let resp = http::Response::builder()
            .status(200)
            .header("Date", Utc::now().to_rfc2822())
            .header("Server", "fsyncd")
            .header("Connection", "close")
            .body("All good, you can close this window ;-)")?;
        server::write_response(resp, writer).await?;

        let access = token_response.access_token().to_owned();

        let mut store = self.inner.token_store.write().await;
        store.push(&token_response).await;

        Ok(access)
    }

    async fn http_client(&self, req: HttpRequest) -> reqwest::Result<HttpResponse> {
        let method = req.method.clone();
        let url = req.url.clone();

        let resp = self
            .inner
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

impl GetToken for Client {
    async fn get_token(&self, scopes: Vec<Scope>) -> anyhow::Result<AccessToken> {
        let cache = self.inner.token_store.read().await.pull(&scopes);
        match cache {
            CacheResult::Ok(access_token) => Ok(access_token),
            CacheResult::Expired(refresh_token, scopes) => {
                self.refresh_token(refresh_token, scopes).await
            }
            CacheResult::None => self.fetch_token(scopes).await,
        }
    }
}

impl PersistCache for Client {
    async fn persist_cache(&self) -> anyhow::Result<()> {
        self.inner.token_store.read().await.flush().await?;
        Ok(())
    }
}

mod server {
    use std::str::{self};

    use anyhow::Context;
    use chrono::Utc;
    use http::{HeaderValue, Method, Request, Uri};
    use tokio::io;

    use super::util::read_until_pattern;

    pub async fn parse_request<R>(reader: R) -> anyhow::Result<Request<Vec<u8>>>
    where
        R: io::AsyncBufRead,
    {
        use io::AsyncReadExt;

        tokio::pin!(reader);

        const DELIM: &[u8; 2] = b"\r\n";

        let mut buf = Vec::new();
        read_until_pattern(&mut reader, DELIM, &mut buf).await?;
        if buf.is_empty() {
            anyhow::bail!("Empty HTTP request");
        }
        let (method, uri) = parse_command(&buf)?;

        let mut req = Request::builder().method(method).uri(uri);

        let mut content_length: Option<usize> = None;
        loop {
            buf.clear();
            read_until_pattern(&mut reader, DELIM, &mut buf).await?;
            if buf.len() <= 2 {
                break;
            }
            let header = parse_header(&buf)?;
            if str::eq_ignore_ascii_case(&header.0, "transfer-encoding") {
                anyhow::bail!("Unsupported header: Transfer-Encoding")
            }
            if str::eq_ignore_ascii_case(&header.0, "content-length") {
                content_length = Some(header.1.parse()?);
            }
            let (name, value) = parse_header(&buf)?;
            req = req.header(name, value.parse::<HeaderValue>()?);
        }
        buf.clear();
        if let Some(len) = content_length {
            if len > buf.capacity() {
                buf.reserve(len - buf.capacity());
            }
            unsafe {
                buf.set_len(len);
            }
            reader.read_exact(&mut buf).await?;
        }
        Ok(req.body(buf)?)
    }

    pub(super) fn parse_command(line: &[u8]) -> anyhow::Result<(Method, Uri)> {
        let mut parts = line.split(|b| *b == b' ');
        let line = str::from_utf8(line)?;

        let method = parts
            .next()
            .with_context(|| format!("no method in header {line}"))?;
        let method = Method::from_bytes(method)
            .with_context(|| format!("Unrecognized method: {}", String::from_utf8_lossy(method)))?;

        let uri = parts
            .next()
            .with_context(|| format!("no path in HTTP header {line}"))?;
        let uri = uri.try_into()?;

        let protocol = parts
            .next()
            .with_context(|| format!("no protocol in HTTP header {line}"))?;
        if protocol != b"HTTP/1.1\r\n" {
            anyhow::bail!("unsupported HTTP protocol in header {line}");
        }
        Ok((method, uri))
    }

    pub(super) fn parse_header(line: &[u8]) -> anyhow::Result<(&str, &str)> {
        let line = str::from_utf8(line)?;
        let (name, value) = line
            .split_once(|b| b == ':')
            .with_context(|| format!("Invalid header: {line}"))?;
        let name = name.trim();
        let value = value.trim();
        Ok((name, value))
    }

    pub async fn write_response<W, B>(resp: http::Response<B>, writer: W) -> anyhow::Result<()>
    where
        W: io::AsyncWrite,
        B: AsRef<[u8]>,
    {
        use io::AsyncWriteExt;

        let (parts, body) = resp.into_parts();

        let has_body = !body.as_ref().is_empty();

        let has_date = parts.headers.contains_key("date");
        let has_server = parts.headers.contains_key("server");
        let has_content_length = parts.headers.contains_key("content-length");

        tokio::pin!(writer);
        writer
            .write(format!("{:?} {}\r\n", parts.version, parts.status).as_bytes())
            .await?;
        if !has_date {
            writer
                .write(format!("Date: {}\r\n", Utc::now().to_rfc2822()).as_bytes())
                .await?;
        }
        if !has_server {
            writer.write(b"Server: fsync::http::server\r\n").await?;
        }
        if has_body && !has_content_length {
            writer
                .write(format!("Content-Length: {}\r\n", body.as_ref().len()).as_bytes())
                .await?;
        }
        for (name, value) in parts.headers.iter() {
            writer.write(format!("{name}: ").as_bytes()).await?;
            writer.write(value.as_bytes()).await?;
            writer.write(b"\r\n").await?;
        }
        writer.write(b"\r\n").await?;
        if has_body {
            writer.write(body.as_ref()).await?;
        }
        writer.flush().await?;
        Ok(())
    }
}

mod util {
    use tokio::io::{self, AsyncReadExt};

    /// Read from reader until either pattern or EOF is found.
    /// Pattern is included in the buffer.
    pub(super) async fn read_until_pattern<R>(
        reader: R,
        pattern: &[u8],
        buf: &mut Vec<u8>,
    ) -> anyhow::Result<usize>
    where
        R: io::AsyncBufRead,
    {
        use io::AsyncBufReadExt;

        debug_assert!(pattern.len() > 0);
        tokio::pin!(reader);
        let mut bb: [u8; 1] = [0];
        let mut len = 0;
        'outer: loop {
            let sz = reader.read_until(pattern[0], buf).await?;
            if sz == 0 {
                break;
            }
            len += sz;
            for c in pattern[1..].iter() {
                let sz = reader.read(&mut bb[..]).await?;
                if sz == 0 {
                    break 'outer;
                }
                len += sz;
                buf.push(bb[0]);
                if bb[0] != *c {
                    continue 'outer;
                }
            }
            break;
        }
        Ok(len)
    }
}

#[cfg(test)]
mod tests {
    use http::Method;

    use super::{server::*, util::*};

    const TEST_REQ: &str = concat!(
        "GET /some/path HTTP/1.1\r\n",
        "User-Agent: fsyncd/13.0\r\n",
        "Content-Length: 12\r\n",
        "\r\n",
        "Request Body",
    );

    #[tokio::test]
    async fn test_read_until_pattern() -> anyhow::Result<()> {
        let expected: &[&[u8]] = &[
            b"GET /some/path HTTP/1.1\r\n",
            b"User-Agent: fsyncd/13.0\r\n",
            b"Content-Length: 12\r\n",
            b"\r\n",
            b"Request Body",
        ];

        let mut cursor = std::io::Cursor::new(TEST_REQ.as_bytes());
        let mut buf = Vec::new();

        for &exp in expected.iter() {
            let res = read_until_pattern(&mut cursor, b"\r\n", &mut buf).await?;
            assert_eq!(res, exp.len());
            assert_eq!(buf.as_slice(), exp);
            buf.clear();
        }

        Ok(())
    }

    #[test]
    fn test_parse_command() -> anyhow::Result<()> {
        let (method, path) = parse_command(b"GET /some/path HTTP/1.1\r\n")?;
        assert_eq!(method, Method::GET);
        assert_eq!(path, "/some/path");
        Ok(())
    }

    #[test]
    fn test_parse_header() -> anyhow::Result<()> {
        let (name, value) = parse_header(b"Content-Length: 12\r\n")?;
        assert_eq!(name, "Content-Length");
        assert_eq!(value, "12");
        assert!(parse_header(b"Content-Length; 12\r\n").is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_parse_request() -> anyhow::Result<()> {
        let req = parse_request(TEST_REQ.as_bytes()).await?;
        assert_eq!(req.method(), Method::GET);
        assert_eq!(req.uri(), "/some/path");
        assert_eq!(req.headers().get("User-Agent").unwrap(), &"fsyncd/13.0");
        assert_eq!(req.headers().get("Content-Length").unwrap(), &"12");
        Ok(())
    }
}
