
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

#[tauri::command]
pub async fn instances_get_all() -> fsync::Result<Vec<Instance>> {
    Instances::get_all().await
}

#[tauri::command]
pub async fn instances_create_config(name: String, local_dir: PathBuf, opts: fsync_client::new::ProviderOpts) -> fsync::Result<()> {
    println!("{name}");
    println!("{local_dir:#?}");
    println!("{opts:#?}");
    Ok(())
}

struct Instances;

impl Instances {
    async fn get_all() -> fsync::Result<Vec<Instance>> {
        let insts = fsync_client::Instance::get_all()?;
        let insts = insts
            .into_iter()
            .map(|i| async {
                let running = i.running();
                let config = i.load_config().await?;
                let provider = config.provider.into();
                let local_dir = config.local_dir;
                let name = i.into_name();
                Ok::<_, anyhow::Error>(Instance {
                    name,
                    running,
                    provider,
                    local_dir,
                })
            });
        let insts = futures::future::try_join_all(insts).await?;
        Ok(insts)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    name: String,
    running: bool,
    provider: fsync::Provider,
    #[type_def(type_of = "String")]
    local_dir: fsync::path::FsPathBuf,
}
