
use serde::{Deserialize, Serialize};

#[tauri::command]
pub async fn instances_get_all() -> fsync::Result<Vec<Instance>> {
    Instances::get_all().await
}

struct Instances;

impl Instances {
    async fn get_all() -> fsync::Result<Vec<Instance>> {
        let insts = fsync_client::Instance::get_all()?;
        let insts = insts
            .into_iter()
            .map(|i| async {
                let daemon_running = i.running();
                let provider = i.load_config().await?.provider.into();
                let name = i.into_name();
                Ok::<_, anyhow::Error>(Instance {
                    name,
                    provider,
                    daemon_running,
                })
            });
        let insts = futures::future::try_join_all(insts).await?;
        Ok(insts)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    name: String,
    provider: fsync::Provider,
    daemon_running: bool,
}
