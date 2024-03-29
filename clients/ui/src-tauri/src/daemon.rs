use std::sync::Arc;

use anyhow::Context;
use fsync::{path::FsPathBuf, FsyncClient};
use fsync_client::Instance;
use serde::{Deserialize, Serialize};
use tokio::{fs, sync::Mutex};

#[tauri::command]
pub async fn daemon_connected(daemon: tauri::State<'_, Daemon>) -> Result<bool, ()> {
    Ok(daemon.connected().await)
}

#[derive(Debug, Serialize, Deserialize)]
struct Persistent {
    instance_name: String,
}

impl Persistent {
    fn disk_file() -> anyhow::Result<FsPathBuf> {
        let dir = dirs::cache_dir().context("Can't get cache directory")?;
        let mut file = FsPathBuf::try_from(dir)?;
        file.push("fsyncui");
        file.push("persistent.json");
        Ok(file)
    }

    async fn load() -> anyhow::Result<Option<Self>> {
        let path = Self::disk_file()?;
        match fs::read(path).await {
            Ok(contents) => {
                let contents = String::from_utf8(contents)?;
                let persistent: Self = serde_json::from_str(&contents)?;
                Ok(Some(persistent))
            }
            Err(_) => Ok(None),
        }
    }

    async fn _save(&self) -> anyhow::Result<()> {
        let path = Self::disk_file()?;
        let dir = path
            .parent()
            .expect("persistent path should have a parent!");
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        let contents = serde_json::to_string(self)?;
        fs::write(&path, contents).await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct Inner {
    _instance_name: String,
    _client: FsyncClient,
}

#[derive(Debug, Default, Clone)]
pub struct Daemon {
    // daemon instance name and client name
    inner: Arc<Mutex<Option<Inner>>>,
}

impl Daemon {
    pub async fn try_auto_connect(&self) {
        let persistent = Persistent::load().await.expect("Should not fail");
        let name = persistent.as_ref().map(|p| p.instance_name.as_str());

        let _ = self.connect(name).await;
    }

    pub async fn connected(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.is_some()
    }

    pub async fn _instance_name(&self) -> Option<String> {
        let inner = self.inner.lock().await;
        inner.as_ref().map(|inner| inner._instance_name.clone())
    }

    pub async fn _client(&self) -> Option<fsync::FsyncClient> {
        let inner = self.inner.lock().await;
        inner.as_ref().map(|inner| inner._client.clone())
    }

    pub async fn connect(&self, name: Option<&str>) -> anyhow::Result<()> {
        let mut instances = Instance::get_all()?;
        instances.retain(|i| i.running());
        let instance = match name {
            Some(name) => instances.into_iter().filter(|i| i.name() == name).next(),
            None => {
                if instances.len() == 1 {
                    instances.into_iter().next()
                } else {
                    None
                }
            }
        };
        let instance =
            instance.with_context(|| format!("Could not find running daemon instance"))?;

        let client = instance.make_client().await?;
        let instance_name = instance.into_name();

        let mut inner = self.inner.lock().await;
        *inner = Some(Inner {
            _instance_name: instance_name,
            _client: client,
        });

        Ok(())
    }
}
