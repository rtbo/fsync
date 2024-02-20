use std::cmp::Ordering;

use dashmap::DashMap;
use fsync::path::{Path, PathBuf};
pub use fsync::tree::{Entry, EntryNode};
use futures::{
    future::{self, BoxFuture},
    StreamExt, TryStreamExt,
};

use crate::storage;

trait EntryExt {
    fn with_local(self, local: fsync::Metadata) -> Self;
    fn with_remote(self, remote: fsync::Metadata) -> Self;
    fn without_local(self) -> Self;
    fn without_remote(self) -> Self;
}

impl EntryExt for Entry {
    fn with_local(self, local: fsync::Metadata) -> Self {
        match self {
            Entry::Remote(remote) => Entry::new_sync(local, remote),
            Entry::Local(..) => Entry::Local(local),
            Entry::Sync { remote, .. } => Entry::new_sync(local, remote),
        }
    }

    fn with_remote(self, remote: fsync::Metadata) -> Self {
        match self {
            Entry::Local(local) => Entry::new_sync(local, remote),
            Entry::Remote(..) => Entry::Remote(remote),
            Entry::Sync { local, .. } => Entry::new_sync(local, remote),
        }
    }

    fn without_local(self) -> Self {
        match self {
            Entry::Local(..) => unreachable!(),
            Entry::Remote(..) => unreachable!(),
            Entry::Sync { remote, .. } => Entry::Remote(remote),
        }
    }

    fn without_remote(self) -> Self {
        match self {
            Entry::Local(..) => unreachable!(),
            Entry::Remote(..) => unreachable!(),
            Entry::Sync { local, .. } => Entry::Local(local),
        }
    }
}

#[derive(Debug)]
pub struct DiffTree {
    nodes: DashMap<PathBuf, EntryNode>,
}

impl DiffTree {
    pub async fn build<L, R>(local: &L, remote: &R) -> anyhow::Result<Self>
    where
        L: storage::Storage,
        R: storage::Storage,
    {
        let nodes = DashMap::new();

        let build = DiffTreeBuild {
            local,
            remote,
            nodes: &nodes,
        };
        build
            .sync(fsync::Metadata::root(), fsync::Metadata::root())
            .await?;

        Ok(Self { nodes })
    }

    pub fn entry(&self, path: &Path) -> Option<EntryNode> {
        self.nodes.get(path).map(|node| node.clone())
    }

    pub fn entries<'a>(
        &'a self,
    ) -> impl Iterator<Item = dashmap::mapref::multiple::RefMulti<'a, PathBuf, EntryNode>> {
        self.nodes.iter()
    }

    pub fn add_local_is_conflict(&self, path: &Path, local: fsync::Metadata) -> bool {
        self.op_entry_is_conflict(path, move |entry| entry.with_local(local))
    }

    pub fn add_remote_is_conflict(&self, path: &Path, remote: fsync::Metadata) -> bool {
        self.op_entry_is_conflict(path, move |entry| entry.with_remote(remote))
    }

    pub fn remove_local(&self, path: &Path) {
        let is_conflict = self.op_entry_is_conflict(path, |entry| entry.without_local());
        debug_assert!(!is_conflict);
    }

    pub fn remove_remote(&self, path: &Path) {
        let is_conflict = self.op_entry_is_conflict(path, |entry| entry.without_remote());
        debug_assert!(!is_conflict);
    }

    /// Apply `op` to entry and return whether it is a conflict
    fn op_entry_is_conflict<F: FnOnce(Entry) -> Entry>(&self, path: &Path, op: F) -> bool {
        let (add, rem) = {
            let mut node = self.nodes.get_mut(path).expect("this node should be valid");

            let rem = if node.entry().is_conflict() { 1 } else { 0 };
            node.op_entry(op);
            let add = if node.entry().is_conflict() { 1 } else { 0 };

            (add, rem)
        };
        if add != rem {
            self.add_conflicts_to_ancestors(path, add - rem);
        }
        add == 1
    }

    fn add_conflicts_to_ancestors(&self, path: &Path, count: isize) {
        let mut parent = path.parent();
        while let Some(path) = parent {
            let mut node = self
                .nodes
                .get_mut(path)
                .expect("parent of valid path should be valid as well");
            node.add_children_conflicts(count);
            parent = path.parent();
        }
    }

    pub fn remove(&self, path: &Path) {
        self.nodes.remove(path);
    }

    pub fn print_out<W>(&self, w: &mut W)
    where
        W: std::io::Write,
    {
        let rootp = Path::root();
        let root = self.nodes.get(rootp);
        if let Some(root) = root {
            for child_name in root.children() {
                let path = rootp.join(child_name);
                self._print_out(w, &path, 0);
            }
        }
    }

    fn _print_out<W>(&self, w: &mut W, path: &Path, indent: usize)
    where
        W: std::io::Write,
    {
        let node = self.nodes.get(path).unwrap();
        let marker = match node.entry() {
            Entry::Sync { .. } => "S",
            Entry::Local { .. } => "L",
            Entry::Remote { .. } => "R",
        };

        writeln!(
            w,
            "{marker} {}{}",
            "  ".repeat(indent),
            path.file_name().unwrap()
        )
        .unwrap();

        for child_name in node.children() {
            let path = path.join(child_name);
            self._print_out(w, &path, indent + 1);
        }
    }
}

struct DiffTreeBuild<'a, L, R> {
    local: &'a L,
    remote: &'a R,
    nodes: &'a DashMap<PathBuf, EntryNode>,
}

impl<'a, L, R> DiffTreeBuild<'a, L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    fn sync(
        &self,
        local: fsync::Metadata,
        remote: fsync::Metadata,
    ) -> BoxFuture<'_, anyhow::Result<usize>> {
        Box::pin(async move {
            let loc_children = entry_children_sorted(&*self.local, &local);
            let rem_children = entry_children_sorted(&*self.remote, &remote);
            let (loc_children, rem_children) = tokio::join!(loc_children, rem_children);

            let loc_children = loc_children?;
            let mut loc_children = loc_children.iter();
            let mut loc_child = loc_children.next();

            let rem_children = rem_children?;
            let mut rem_children = rem_children.iter();
            let mut rem_child = rem_children.next();

            let mut children = Vec::new();
            let mut joinvec = Vec::new();

            loop {
                match (loc_child, rem_child) {
                    (None, None) => break,
                    (Some(loc), Some(rem)) => match loc.name().cmp(rem.name()) {
                        Ordering::Equal => {
                            joinvec.push(self.sync(loc.clone(), rem.clone()));
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

            let conflicts = future::try_join_all(joinvec).await?.into_iter().sum();

            assert_eq!(local.path(), remote.path());
            let path = local.path().to_owned();
            let entry = Entry::new_sync(local, remote);
            let res = conflicts + if entry.is_conflict() { 1 } else { 0 };

            let node = EntryNode::new(entry, children, conflicts);
            self.nodes.insert(path, node);

            Ok(res)
        })
    }

    fn local(&self, entry: fsync::Metadata) -> BoxFuture<'_, anyhow::Result<usize>> {
        Box::pin(async move {
            let mut child_names = Vec::new();

            let conflicts = if entry.is_dir() {
                let mut joinvec = Vec::new();
                let children = self.local.dir_entries(entry.path());
                tokio::pin!(children);

                while let Some(child) = children.next().await {
                    let child = child?;
                    child_names.push(child.name().to_owned());
                    joinvec.push(self.local(child));
                }
                future::try_join_all(joinvec).await?.into_iter().sum()
            } else {
                0
            };

            let path = entry.path().to_owned();
            let entry = Entry::Local(entry);
            let node = EntryNode::new(entry, child_names, conflicts);
            self.nodes.insert(path, node);
            Ok(conflicts)
        })
    }

    fn remote(&self, entry: fsync::Metadata) -> BoxFuture<'_, anyhow::Result<usize>> {
        Box::pin(async move {
            let mut child_names = Vec::new();

            let conflicts = if entry.is_dir() {
                let mut joinvec = Vec::new();
                let children = self.remote.dir_entries(entry.path());
                tokio::pin!(children);

                while let Some(child) = children.next().await {
                    let child = child?;
                    child_names.push(child.name().to_owned());
                    joinvec.push(self.remote(child));
                }
                future::try_join_all(joinvec).await?.into_iter().sum()
            } else {
                0
            };

            let path = entry.path().to_owned();
            let entry = Entry::Remote(entry);
            let node = EntryNode::new(entry, child_names, conflicts);
            self.nodes.insert(path, node);
            Ok(conflicts)
        })
    }
}

async fn entry_children_sorted<S>(
    storage: &S,
    entry: &fsync::Metadata,
) -> anyhow::Result<Vec<fsync::Metadata>>
where
    S: storage::Storage,
{
    if !entry.is_dir() {
        return Ok(vec![]);
    }
    let children = storage.dir_entries(entry.path());
    let mut children = children.try_collect::<Vec<_>>().await?;

    children.sort_unstable_by(|a, b| a.name().cmp(b.name()));

    Ok(children)
}
