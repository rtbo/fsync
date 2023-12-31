use std::pin::Pin;
use std::str;

use camino::Utf8Path;
use futures::Future;
use yup_oauth2::authenticator::HyperClientBuilder;
use yup_oauth2::authenticator_delegate::{DefaultInstalledFlowDelegate, InstalledFlowDelegate};

use crate::http;

pub type Authenticator = yup_oauth2::authenticator::Authenticator<http::Connector>;
pub use yup_oauth2::{parse_application_secret, AccessToken, ApplicationSecret};

pub struct Params<'a> {
    pub app_secret: ApplicationSecret,
    pub token_cache_path: &'a Utf8Path,
}

pub async fn save_secret(path: &Utf8Path, app_secret: &ApplicationSecret) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(app_secret)?;
    tokio::fs::write(path, &json).await?;
    Ok(())
}

pub async fn load_secret(path: &Utf8Path) -> anyhow::Result<ApplicationSecret> {
    let json = tokio::fs::read(path).await?;
    let json = str::from_utf8(&json)?;
    Ok(serde_json::from_str(json)?)
}

pub async fn installed_flow<C>(
    oauth2_params: Params<'_>,
    client: C,
) -> std::io::Result<Authenticator>
where
    C: HyperClientBuilder<Connector = http::Connector>,
{
    let auth = yup_oauth2::InstalledFlowAuthenticator::with_client(
        oauth2_params.app_secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        client,
    )
    .persist_tokens_to_disk(oauth2_params.token_cache_path)
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
