use oauth2::{AuthUrl, ClientId, ClientSecret, TokenUrl};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::path::FsPath;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub auth_url: AuthUrl,
    pub token_url: TokenUrl,
}

#[derive(Debug)]
pub struct Params<'a> {
    pub secret: Secret,
    pub token_cache_path: &'a FsPath,
}

pub async fn load_google_secret(path: &FsPath) -> anyhow::Result<Secret> {
    let json = fs::read(path).await?;
    let goog: GoogleAppSecret = serde_json::from_slice(&json)?;
    match goog {
        GoogleAppSecret::Installed(secret) => Ok(Secret {
            client_id: ClientId::new(secret.client_id),
            client_secret: ClientSecret::new(secret.client_secret),
            auth_url: AuthUrl::new(secret.auth_uri)?,
            token_url: TokenUrl::new(secret.token_uri)?,
        }),
        GoogleAppSecret::Web(_) => anyhow::bail!(
            "Wrong kind of secret file. Please get a secret file with an \"installed\" field"
        ),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoogleSecret {
    client_id: String,
    client_secret: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    redirect_uris: Vec<String>,
    auth_uri: String,
    token_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_provider_x509_cert_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_x509_cert_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum GoogleAppSecret {
    Installed(GoogleSecret),
    Web(GoogleSecret),
}

#[test]
fn test_google_secret_serialization() -> anyhow::Result<()> {
    let secret = GoogleAppSecret::Installed(GoogleSecret {
        client_id: "client id".to_string(),
        client_secret: "client secret".to_string(),
        redirect_uris: vec!["redirect uri".to_string()],
        auth_uri: "auth uri".to_string(),
        token_uri: "token uri".to_string(),
        client_email: None,
        auth_provider_x509_cert_url: None,
        client_x509_cert_url: None,
    });
    let json = serde_json::to_string_pretty(&secret)?;
    const EXPECTED: &str = r#"{
  "installed": {
    "client_id": "client id",
    "client_secret": "client secret",
    "redirect_uris": [
      "redirect uri"
    ],
    "auth_uri": "auth uri",
    "token_uri": "token uri"
  }
}"#;
    assert_eq!(json, EXPECTED);
    Ok(())
}
