use std::net::SocketAddr;

use anyhow::Context;
use chrono::Utc;
use oauth2::{
    basic::BasicTokenResponse, AuthorizationCode, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope,
};
use tokio::{io, net};

use super::{server, Client};
use crate::uri;

impl Client {
    pub(super) async fn fetch_token_pkce(
        &self,
        scopes: Vec<Scope>,
    ) -> anyhow::Result<BasicTokenResponse> {
        log::info!("Starting PKCE flow for scopes {scopes:?}");

        let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
        let listener = net::TcpListener::bind(&addr).await?;
        let redirect_addr = listener.local_addr()?;

        let redirect_url = RedirectUrl::new(format!("http://{redirect_addr}"))?;
        let redirect_url = std::borrow::Cow::Borrowed(&redirect_url);

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let (auth_url, csrf_state) = self
            .inner
            .oauth2
            .authorize_url(CsrfToken::new_random)
            .set_redirect_uri(redirect_url.clone())
            .add_scopes(scopes)
            .set_pkce_challenge(pkce_challenge)
            .url();

        println!("Browser should window should open for authentification.");
        println!("If it doesn't navigate to {auth_url}.");

        tokio::task::spawn_blocking(move || webbrowser::open(auth_url.as_str()));

        log::trace!("starting local server on {redirect_addr}");
        let (socket, addr) = listener.accept().await?;

        log::trace!("incoming request from {addr:#?}");
        let (reader, writer) = io::split(socket);
        let reader = io::BufReader::new(reader);
        let writer = io::BufWriter::new(writer);
        let req = server::parse_request(reader).await?;
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

        log::trace!("PKCE code: {code:#?}");

        if state.secret() != csrf_state.secret() {
            log::error!("Failed PKCE challenge");
            let resp = http::Response::builder()
                .status(401)
                .header("Date", Utc::now().to_rfc2822())
                .header("Server", "fsyncd")
                .header("Connection", "close")
                .body("Could not verify the CSRF token :-(")?;
            server::write_response(resp, writer).await?;
            anyhow::bail!("Could not verify the CSRF token");
        }

        log::trace!("exchanging code for token");

        let token_response = self
            .inner
            .oauth2
            .exchange_code(code)
            .set_pkce_verifier(pkce_verifier)
            .set_redirect_uri(redirect_url)
            .request_async(|req| async { self.http(req).await })
            .await?;

        let resp = http::Response::builder()
            .status(200)
            .header("Date", Utc::now().to_rfc2822())
            .header("Server", "fsyncd")
            .header("Connection", "close")
            .body("All good, you can close this window ;-)")?;
        server::write_response(resp, writer).await?;

        Ok(token_response)
    }
}
