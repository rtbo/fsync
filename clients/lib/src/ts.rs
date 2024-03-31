use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

pub type Types = (
    fsync::Provider,
    Instance,
    crate::config::drive::SecretOpts,
    crate::config::drive::Opts,
    crate::config::ProviderOpts,
);

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    name: String,
    running: bool,
    provider: fsync::Provider,
    #[type_def(type_of = "String")]
    local_dir: fsync::path::FsPathBuf,
}

impl Instance {
    pub async fn new_from(instance: crate::Instance) -> fsync::Result<Self> {
        let running = instance.running();
        let config = instance.load_config().await?;
        let provider = config.provider.into();
        let local_dir = config.local_dir;
        let name = instance.into_name();
        Ok(Self {
            name,
            running,
            provider,
            local_dir,
        })
    }
}

impl Instance {
    pub async fn get_all() -> fsync::Result<Vec<Instance>> {
        let insts = crate::Instance::get_all()?;
        let insts = insts.into_iter().map(Instance::new_from);
        let insts = futures::future::try_join_all(insts).await?;
        Ok(insts)
    }
}
