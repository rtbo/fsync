use std::pin::Pin;
use std::str;

use camino::{Utf8Path, Utf8PathBuf};
use futures::Future;
use yup_oauth2::authenticator_delegate::{DefaultInstalledFlowDelegate, InstalledFlowDelegate};
use yup_oauth2::hyper::client::HttpConnector;
use yup_oauth2::hyper_rustls::HttpsConnector;
use yup_oauth2::ApplicationSecret;

pub type Connector = HttpsConnector<HttpConnector>;
pub type Authenticator = yup_oauth2::authenticator::Authenticator<Connector>;

pub fn secret_path(config_dir: &Utf8Path) -> Utf8PathBuf {
    config_dir.join("client_secret.json")
}

pub fn token_cache_path(cache_dir: &Utf8Path) -> Utf8PathBuf {
    cache_dir.join("token_cache.json")
}

pub async fn save_secret(path: &Utf8Path, app_secret: &ApplicationSecret) -> crate::Result<()> {
    let json = serde_json::to_string(app_secret)?;
    tokio::fs::write(path, &json).await?;
    Ok(())
}

pub async fn load_secret(path: &Utf8Path) -> crate::Result<ApplicationSecret> {
    let json = tokio::fs::read(path).await?;
    let json = str::from_utf8(&json)?;
    Ok(serde_json::from_str(json)?)
}

pub async fn installed_flow(
    app_secret: ApplicationSecret,
    token_cache_path: &Utf8Path,
) -> crate::Result<Authenticator> {
    let auth = yup_oauth2::InstalledFlowAuthenticator::builder(
        app_secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .persist_tokens_to_disk(token_cache_path)
    .flow_delegate(Box::new(InstalledFlowBrowserDelegate))
    .build()
    .await?;
    Ok(auth)
}

async fn browser_user_url(url: &str, need_code: bool) -> Result<String, String> {
    if webbrowser::open(url).is_ok() {
        println!("webbrowser was successfully opened.");
    }
    let def_delegate = DefaultInstalledFlowDelegate;
    def_delegate.present_user_url(url, need_code).await
}

/// our custom delegate struct we will implement a flow delegate trait for:
/// in this case we will implement the `InstalledFlowDelegated` trait
#[derive(Copy, Clone)]
struct InstalledFlowBrowserDelegate;

/// here we implement only the present_user_url method with the added webbrowser opening
/// the other behaviour of the trait does not need to be changed.
impl InstalledFlowDelegate for InstalledFlowBrowserDelegate {
    /// the actual presenting of URL and browser opening happens in the function defined above here
    /// we only pin it
    fn present_user_url<'a>(
        &'a self,
        url: &'a str,
        need_code: bool,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(browser_user_url(url, need_code))
    }
}
