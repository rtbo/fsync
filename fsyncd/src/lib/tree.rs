use std::{cmp::Ordering, mem};

use dashmap::DashMap;
pub use fsync::tree::{Entry, EntryNode};
use fsync::{
    path::{Path, PathBuf},
    stat, StorageLoc,
};
use futures::{
    future::{self, BoxFuture},
    StreamExt, TryStreamExt,
};

use crate::storage;

trait EntryExt {
    fn with(self, md: fsync::Metadata, loc: StorageLoc) -> Self;
    fn with_local(self, local: fsync::Metadata) -> Self;
    fn with_remote(self, remote: fsync::Metadata) -> Self;
    fn without(self, loc: StorageLoc) -> Self;
    fn without_local(self) -> Self;
    fn without_remote(self) -> Self;
}

impl EntryExt for Entry {
    fn with(self, md: fsync::Metadata, loc: StorageLoc) -> Self {
        match loc {
            StorageLoc::Local => self.with_local(md),
            StorageLoc::Remote => self.with_remote(md),
        }
    }

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

    fn without(self, loc: StorageLoc) -> Self {
        match loc {
            StorageLoc::Local => self.without_local(),
            StorageLoc::Remote => self.without_remote(),
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

    pub fn has_entry(&self, path: &Path) -> bool {
        self.nodes.get(path).is_some()
    }

    pub fn entry(&self, path: &Path) -> Option<EntryNode> {
        self.nodes.get(path).map(|node| node.clone())
    }

    pub fn entries<'a>(
        &'a self,
    ) -> impl Iterator<Item = dashmap::mapref::multiple::RefMulti<'a, PathBuf, EntryNode>> {
        self.nodes.iter()
    }

    pub fn insert(&self, path: &Path, entry: EntryNode) {
        debug_assert_eq!(path, entry.path());
        debug_assert!(!self.has_entry(path));
        let parent_path = path.parent().expect("This path should have a parent");
        {
            let mut parent = self
                .nodes
                .get_mut(parent_path)
                .expect("this parent should be valid");
            parent.add_child(
                path.file_name()
                    .expect("this path should have a file name")
                    .to_string(),
            );
            parent.add_stat(&entry.stats());
        }
        self.nodes.insert(path.to_path_buf(), entry);
    }

    pub fn add_to_storage_check_conflict(
        &self,
        path: &Path,
        metadata: fsync::Metadata,
        loc: StorageLoc,
    ) -> bool {
        self.op_entry_check_conflict(path, |entry| entry.with(metadata, loc))
    }

    pub fn remove_from_storage(&self, path: &Path, loc: StorageLoc) {
        let stat_diff = {
            let mut node = self.nodes.get_mut(path).expect("This node should be valid");
            if node.is_sync() {
                let rem = node.stats();
                node.op_entry(|entry| entry.without(loc));
                let add = node.stats();
                add - rem
            } else {
                let stat = node.stats();
                mem::drop(node);
                self.nodes.remove(path);
                let parent_path = path.parent().expect("This path should have a valid parent");
                let file_name = path
                    .file_name()
                    .expect("This path should have a valid name");
                let mut parent = self.nodes.get_mut(parent_path).unwrap();
                parent.remove_child(file_name);
                -stat
            }
        };
        self.add_stat_to_ancestors(path, &stat_diff);
    }

    /// Apply `op` to entry and return whether it is a conflict
    fn op_entry_check_conflict<F: FnOnce(Entry) -> Entry>(&self, path: &Path, op: F) -> bool {
        let (stat_diff, is_conflict) = {
            let mut node = self.nodes.get_mut(path).expect("this node should be valid");

            let rem = node.stats();
            node.op_entry(op);
            let add = node.stats();

            (add - rem, node.entry().is_conflict())
        };
        if !stat_diff.is_null() {
            self.add_stat_to_ancestors(path, &stat_diff);
        }
        is_conflict
    }

    fn add_stat_to_ancestors(&self, path: &Path, diff: &stat::Tree) {
        let mut parent = path.parent();
        while let Some(path) = parent {
            let mut node = self
                .nodes
                .get_mut(path)
                .expect("parent of valid path should be valid as well");
            node.add_stat(diff);
            parent = path.parent();
        }
    }

    /// Ensure that parents of `path` are added in the tree for `loc`.
    /// Also perform stats calculation.
    /// Returns which of the parents are conflicts.
    pub fn ensure_parents(&self, path: &Path, loc: StorageLoc) -> Vec<(PathBuf, bool)> {
        debug_assert!(path.is_absolute());
        let mut conflicts = vec![];
        if path.is_root() {
            return conflicts;
        }

        let mut dir_stat = stat::Dir::null();
        let mut tree_stat = stat::Tree::null();

        let mut parent = path.parent();
        while let Some(path) = parent {
            let mut node = self.nodes.get_mut(path).expect("this node should be valid");

            if node.entry().is_at_loc(loc) {
                node.add_stat(&tree_stat);
            } else {
                let md = fsync::Metadata::Directory {
                    path: path.to_path_buf(),
                    stat: Some(dir_stat),
                };

                let bef = node.stats();
                node.op_entry(move |entry| entry.with(md, loc));
                let aft = node.stats();

                let is_conflict = node.entry().is_conflict();
                conflicts.push((path.to_path_buf(), is_conflict));

                tree_stat = aft - bef;
                dir_stat += *tree_stat.by_loc(loc);
            }
            parent = path.parent();
        }
        conflicts
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
    ) -> BoxFuture<'_, anyhow::Result<stat::Tree>> {
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

            let mut children_stat = stat::Tree::null();
            let stat_vec = future::try_join_all(joinvec).await?;
            for stat in stat_vec {
                children_stat = children_stat + stat;
            }

            assert_eq!(local.path(), remote.path());
            let path = local.path().to_owned();
            let entry = Entry::new_sync(local, remote);
            let node = EntryNode::new(entry, children, children_stat);
            let res = node.stats();

            self.nodes.insert(path, node);

            Ok(res)
        })
    }

    fn local(&self, entry: fsync::Metadata) -> BoxFuture<'_, anyhow::Result<stat::Tree>> {
        Box::pin(async move {
            let mut children_names = Vec::new();
            let mut children_stat = stat::Tree::null();

            if let fsync::Metadata::Directory { path, .. } = &entry {
                let mut joinvec = Vec::new();
                let children = self.local.dir_entries(&path, None);
                tokio::pin!(children);

                while let Some(child) = children.next().await {
                    let child = child?;
                    children_names.push(child.name().to_owned());
                    joinvec.push(self.local(child));
                }

                let stat_vec = future::try_join_all(joinvec).await?;
                for s in stat_vec {
                    children_stat = children_stat + s;
                }
            }

            let path = entry.path().to_owned();
            let entry = Entry::Local(entry);
            let node = EntryNode::new(entry, children_names, children_stat);
            let res = node.stats();

            self.nodes.insert(path, node);

            Ok(res)
        })
    }

    fn remote(&self, entry: fsync::Metadata) -> BoxFuture<'_, anyhow::Result<stat::Tree>> {
        Box::pin(async move {
            let mut child_names = Vec::new();
            let mut children_stat = stat::Tree::null();

            if let fsync::Metadata::Directory { path, .. } = &entry {
                let mut joinvec = Vec::new();
                let children = self.remote.dir_entries(path, None);
                tokio::pin!(children);

                while let Some(child) = children.next().await {
                    let child = child?;
                    child_names.push(child.name().to_owned());
                    joinvec.push(self.remote(child));
                }

                let stat_vec = future::try_join_all(joinvec).await?;
                for s in stat_vec {
                    children_stat = children_stat + s;
                }
            }

            let path = entry.path().to_owned();
            let entry = Entry::Remote(entry);
            let node = EntryNode::new(entry, child_names, children_stat);
            let res = node.stats();

            self.nodes.insert(path, node);

            Ok(res)
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
    let children = storage.dir_entries(entry.path(), None);
    let mut children = children.try_collect::<Vec<_>>().await?;

    children.sort_unstable_by(|a, b| a.name().cmp(b.name()));

    Ok(children)
}
