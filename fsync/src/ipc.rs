use camino::Utf8PathBuf;

use crate::difftree::TreeNode;

#[tarpc::service]
pub trait Fsync {
    async fn entry(path: Option<Utf8PathBuf>) -> Option<TreeNode>;
}
