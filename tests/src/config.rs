use fsync::path::FsPath;
use fsyncd::storage::id::IdBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountKey {
    #[serde(rename = "type")]
    pub typ: String,
    pub project_id: String,
    pub private_key_id: String,
    pub private_key: String,
    pub client_email: String,
    pub client_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub auth_provider_x509_cert_url: String,
    pub client_x509_cert_url: String,
    pub universe_domain: String,
}

pub async fn load_account_key() -> anyhow::Result<AccountKey> {
    let mut json_key = std::env::var("GCP_SERVICE_ACCOUNT_JSON_KEY").ok();
    if json_key.is_none() {
        let keypath = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("gcp_key.json");
        json_key = Some(tokio::fs::read_to_string(&keypath).await?);
    }
    let json_key = json_key.unwrap();
    let account_key: AccountKey = serde_json::from_str(&json_key)?;
    Ok(account_key)
}

pub async fn load_drive_root_id() -> anyhow::Result<IdBuf> {
    let mut id = std::env::var("TEST_DRIVE_ROOT_ID").ok();
    if id.is_none() {
        let path = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("drive_root.id");
        id = Some(tokio::fs::read_to_string(&path).await?);
    }
    let id = id.unwrap();
    Ok(IdBuf::from(id))
}
