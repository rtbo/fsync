use camino::Utf8PathBuf;

use crate::tree;

#[tarpc::service]
pub trait Fsync {
    async fn entry(path: Option<Utf8PathBuf>) -> Option<tree::Node>;
    async fn copy_remote_to_local(path: Utf8PathBuf) -> Result<(), String>;
}
