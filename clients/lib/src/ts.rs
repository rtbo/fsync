use fsync::{path::PathBuf, stat};
use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

pub type Types = (
    fsync::Error,
    fsync::Provider,
    fsync::StorageDir,
    fsync::StorageLoc,
    fsync::Operation,
    fsync::Progress,
    PathProgress,
    Instance,
    crate::config::drive::SecretOpts,
    crate::config::drive::Opts,
    crate::config::ProviderOpts,
    fsync::Metadata,
    EntryType,
    TreeEntry,
    NodeAndChildren,
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

#[derive(Debug, Copy, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum EntryType {
    /// Entry is a directory
    Directory,
    /// Entry is a regular file
    Regular,
    /// Entry type is not consistent accross remote and local storage
    Inconsistent,
}

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    pub path: PathBuf,
    pub name: Option<String>,
    pub entry: fsync::tree::Entry,
    pub children: Vec<String>,
    pub children_node_stat: stat::Node,
}

impl From<fsync::tree::EntryNode> for TreeEntry {
    fn from(value: fsync::tree::EntryNode) -> Self {
        let path = value.path().to_owned();
        let name = path.file_name().map(|s| s.to_owned());
        let (entry, children, children_node_stat) = value.into_parts();
        TreeEntry {
            path,
            name,
            entry,
            children,
            children_node_stat,
        }
    }
}

/// A struct gathering a node and its children
#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub struct NodeAndChildren {
    pub node: TreeEntry,
    pub children: Vec<TreeEntry>,
}

/// A progress struct
#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub struct PathProgress {
    path: PathBuf,
    progress: fsync::Progress,
}

impl From<(PathBuf, fsync::Progress)> for PathProgress {
    fn from((path, progress): (PathBuf, fsync::Progress)) -> Self {
        Self { path, progress }
    }
}
