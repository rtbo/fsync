use camino::Utf8PathBuf;
use crate::difftree::TreeNode;

#[tarpc::service]
pub trait Fsync {
    async fn entry(path: Utf8PathBuf) -> Option<TreeNode>;
}
