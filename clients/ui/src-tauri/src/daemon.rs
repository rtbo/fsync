use std::sync::Arc;

use anyhow::Context;
use fsync::{path::FsPathBuf, FsyncClient};
use fsync_client::Instance;
use serde::{Deserialize, Serialize};
use tokio::{fs, sync::Mutex};

#[tauri::command]
pub async fn connected(daemon: tauri::State<'_, Daemon>) -> Result<bool, ()> {
    Ok(daemon.connected().await)
}

#[tauri::command]
async fn _get_all_instances(daemon: tauri::State<'_, Daemon>) -> fsync::Result<Vec<String>> {
    Ok(daemon._all_instances().await)
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

#[derive(Debug, Clone, Default)]
struct Inner {
    client: Option<(String, FsyncClient)>,
    all_instances: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Daemon {
    // daemon instance name and client name
    inner: Arc<Mutex<Inner>>,
}

impl Daemon {
    pub async fn try_auto_connect(&self) {
        let persistent = Persistent::load().await.expect("Should not fail");
        let name = persistent.as_ref().map(|p| p.instance_name.as_str());

        let _ = self.connect(name).await;
    }

    pub async fn connected(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.client.is_some()
    }

    pub async fn _instance_name(&self) -> Option<String> {
        let inner = self.inner.lock().await;
        inner.client.as_ref().map(|(name, _)| name.clone())
    }

    pub async fn _client(&self) -> Option<fsync::FsyncClient> {
        let inner = self.inner.lock().await;
        inner.client.as_ref().map(|(_, client)| client.clone())
    }

    pub async fn _all_instances(&self) -> Vec<String> {
        let inner = self.inner.lock().await;
        inner.all_instances.clone()
    }

    pub async fn connect(&self, name: Option<&str>) -> anyhow::Result<()> {
        let instances = Instance::get_all()?;
        let instance = match name {
            Some(name) => instances.iter().filter(|i| i.name() == name).next(),
            None => {
                if instances.len() == 1 {
                    instances.iter().next()
                } else {
                    None
                }
            }
        };
        let instance =
            instance.with_context(|| format!("Could not find running daemon instance"))?;

        let client = instance.make_client().await?;
        let instance_name = instance.name().to_string();
        let all_instances: Vec<String> = instances.into_iter().map(|i| i.into_name()).collect();

        let mut inner = self.inner.lock().await;
        inner.client = Some((instance_name, client));
        inner.all_instances = all_instances;

        Ok(())
    }
}
