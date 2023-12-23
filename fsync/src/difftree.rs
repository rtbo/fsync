use std::cmp::Ordering;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use dashmap::DashMap;
use futures::{
    future::{self, BoxFuture},
    StreamExt, TryStreamExt,
};
use serde::{Deserialize, Serialize};

use crate::{Entry, Result, Storage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TreeEntry {
    Local(Entry),
    Remote(Entry),
    Both { local: Entry, remote: Entry },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    entry: TreeEntry,
    children: Vec<String>,
}

#[derive(Debug)]
pub struct DiffTree {
    nodes: Arc<DashMap<Utf8PathBuf, TreeNode>>,
}

impl DiffTree {
    pub async fn from_cache<L, R>(local: Arc<L>, remote: Arc<R>) -> Result<Self>
    where
        L: Storage + Send + Sync + 'static,
        R: Storage + Send + Sync + 'static,
    {
        let nodes = Arc::new(DashMap::new());

        let build = DiffTreeBuild {
            local,
            remote,
            nodes: nodes.clone(),
        };
        build.both(None).await?;

        Ok(Self { nodes })
    }

    pub fn entry(&self, path: &Utf8Path) -> Option<TreeNode> {
        self.nodes.get(path).map(|node| node.clone())
    }

    pub fn print_out(&self) {
        let root = self.nodes.get(Utf8Path::new(""));
        if let Some(root) = root {
            for child_name in &root.children {
                let path = Utf8Path::new(child_name);
                self._print_out(path, 0);
            }
        }
    }

    fn _print_out(&self, path: &Utf8Path, indent: usize) {
        let node = self.nodes.get(path).unwrap();
        let marker = match node.entry {
            TreeEntry::Both { .. } => "B",
            TreeEntry::Local { .. } => "L",
            TreeEntry::Remote { .. } => "R",
        };

        println!(
            "{marker} {}{}",
            "  ".repeat(indent),
            path.file_name().unwrap()
        );

        for child_name in &node.children {
            let path = path.join(child_name);
            self._print_out(&path, indent + 1);
        }
    }
}

struct DiffTreeBuild<L, R> {
    local: Arc<L>,
    remote: Arc<R>,
    nodes: Arc<DashMap<Utf8PathBuf, TreeNode>>,
}

impl<L, R> DiffTreeBuild<L, R>
where
    L: Storage + Send + Sync + 'static,
    R: Storage + Send + Sync + 'static,
{
    fn both(&self, both: Option<(Entry, Entry)>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let loc_path_id = both.as_ref().map(|b| b.0.path_id());
            let loc_children = self.local.entries(loc_path_id);
            let loc_children = loc_children.try_collect::<Vec<_>>();

            let rem_path_id = both.as_ref().map(|b| b.1.path_id());
            let rem_children = self.remote.entries(rem_path_id);
            let rem_children = rem_children.try_collect::<Vec<_>>();

            let (loc_children, rem_children) = tokio::join!(loc_children, rem_children);

            let mut loc_children = loc_children?;
            loc_children.sort_unstable_by(|a, b| a.name().cmp(b.name()));
            let mut loc_children = loc_children.iter();
            let mut loc_child = loc_children.next();

            let mut rem_children = rem_children?;
            rem_children.sort_unstable_by(|a, b| a.name().cmp(b.name()));
            let mut rem_children = rem_children.iter();
            let mut rem_child = rem_children.next();

            let mut children = Vec::new();
            let mut joinvec = Vec::new();

            loop {
                match (loc_child, rem_child) {
                    (None, None) => break,
                    (Some(loc), Some(rem)) => match loc.name().cmp(rem.name()) {
                        Ordering::Equal => {
                            match (loc.is_dir(), rem.is_dir()) {
                                (true, true) | (false, false) => {
                                    joinvec.push(self.both(Some((loc.clone(), rem.clone()))));
                                }
                                (true, false) => {
                                    joinvec.push(self.local(loc.clone()));
                                }
                                (false, true) => {
                                    joinvec.push(self.remote(rem.clone()));
                                }
                            }
                            children.push(loc.name().to_string());
                            loc_child = loc_children.next();
                            rem_child = rem_children.next();
                        }
                        Ordering::Less => {
                            joinvec.push(self.local(loc.clone()));
                            children.push(loc.name().to_string());
                            loc_child = loc_children.next();
                        }
                        Ordering::Greater => {
                            joinvec.push(self.remote(rem.clone()));
                            children.push(rem.name().to_string());
                            rem_child = rem_children.next();
                        }
                    },
                    (Some(loc), None) => {
                        joinvec.push(self.local(loc.clone()));
                        children.push(loc.name().to_string());
                        loc_child = loc_children.next();
                    }
                    (None, Some(rem)) => {
                        joinvec.push(self.remote(rem.clone()));
                        children.push(rem.name().to_string());
                        rem_child = rem_children.next();
                    }
                }
            }

            future::try_join_all(joinvec).await?;

            let (path, entry) = if let Some((local, remote)) = both {
                assert_eq!(local.path(), remote.path());
                let path = local.path().to_owned();
                (path, TreeEntry::Both { local, remote })
            } else {
                (
                    Utf8PathBuf::default(),
                    TreeEntry::Both {
                        local: Entry::default(),
                        remote: Entry::default(),
                    },
                )
            };

            let node = TreeNode { entry, children };
            self.nodes.insert(path, node);

            Ok(())
        })
    }

    fn local(&self, entry: Entry) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut child_names = Vec::new();
            let mut joinvec = Vec::new();

            {
                let children = self.local.entries(Some(entry.path_id()));
                tokio::pin!(children);

                while let Some(child) = children.next().await {
                    let child = child?;
                    child_names.push(child.name().to_owned());
                    joinvec.push(self.local(child));
                }
            }

            future::try_join_all(joinvec).await?;

            let path = entry.path().to_owned();
            let entry = TreeEntry::Local(entry);
            let node = TreeNode {
                entry,
                children: child_names,
            };
            self.nodes.insert(path, node);
            Ok(())
        })
    }

    fn remote(&self, entry: Entry) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut child_names = Vec::new();
            let mut joinvec = Vec::new();

            {
                let children = self.remote.entries(Some(entry.path_id()));
                tokio::pin!(children);

                while let Some(child) = children.next().await {
                    let child = child?;
                    child_names.push(child.name().to_owned());
                    joinvec.push(self.remote(child));
                }
            }

            future::try_join_all(joinvec).await?;

            let path = entry.path().to_owned();
            let entry = TreeEntry::Remote(entry);
            let node = TreeNode {
                entry,
                children: child_names,
            };
            self.nodes.insert(path, node);
            Ok(())
        })
    }
}
