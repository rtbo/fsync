use std::cmp::Ordering;

use dashmap::DashMap;
use fsync::path::{Path, PathBuf};
use futures::{
    future::{self, BoxFuture},
    StreamExt, TryStreamExt,
};
use serde::{Deserialize, Serialize};

use crate::storage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Entry {
    Local(fsync::Metadata),
    Remote(fsync::Metadata),
    Both {
        local: fsync::Metadata,
        remote: fsync::Metadata,
    },
}

impl Entry {
    fn with_remote(self, remote: fsync::Metadata) -> Self {
        match self {
            Entry::Local(local) => Entry::Both { local, remote },
            Entry::Remote(..) => Entry::Remote(remote),
            Entry::Both { local, .. } => Entry::Both { local, remote },
        }
    }

    fn with_local(self, local: fsync::Metadata) -> Self {
        match self {
            Entry::Remote(remote) => Entry::Both { local, remote },
            Entry::Local(..) => Entry::Local(local),
            Entry::Both { remote, .. } => Entry::Both { local, remote },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    entry: Entry,
    children: Vec<String>,
}

impl Node {
    pub fn new(entry: Entry, children: Vec<String>) -> Self {
        Self { entry, children }
    }

    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn children(&self) -> &[String] {
        &self.children
    }

    pub fn path(&self) -> &Path {
        match &self.entry {
            Entry::Both { local, remote } => {
                debug_assert_eq!(local.path(), remote.path());
                local.path()
            }
            Entry::Local(entry) => entry.path(),
            Entry::Remote(entry) => entry.path(),
        }
    }

    pub fn is_local_only(&self) -> bool {
        matches!(self.entry, Entry::Local(..))
    }

    pub fn is_remote_only(&self) -> bool {
        matches!(self.entry, Entry::Remote(..))
    }

    pub fn is_both(&self) -> bool {
        matches!(self.entry, Entry::Both { .. })
    }

    pub fn add_local(&mut self, local: fsync::Metadata) {
        use std::mem;
        let invalid: Entry = unsafe { mem::MaybeUninit::zeroed().assume_init() };
        let valid = mem::replace(&mut self.entry, invalid);
        self.entry = valid.with_local(local);
    }

    pub fn add_remote(&mut self, remote: fsync::Metadata) {
        use std::mem;
        let invalid: Entry = unsafe { mem::MaybeUninit::zeroed().assume_init() };
        let valid = mem::replace(&mut self.entry, invalid);
        self.entry = valid.with_remote(remote);
    }
}

impl From<Entry> for fsync::tree::Entry {
    fn from(value: Entry) -> Self {
        match value {
            Entry::Local(metadata) => fsync::tree::Entry::Local(metadata),
            Entry::Remote(metadata) => fsync::tree::Entry::Remote(metadata),
            Entry::Both { local, remote } => fsync::tree::Entry::Both { local, remote },
        }
    }
}

impl From<Node> for fsync::tree::Node {
    fn from(value: Node) -> Self {
        fsync::tree::Node::new(value.entry.into(), value.children)
    }
}

#[derive(Debug)]
pub struct DiffTree {
    nodes: DashMap<PathBuf, Node>,
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
            .both(fsync::Metadata::root(), fsync::Metadata::root())
            .await?;

        Ok(Self { nodes })
    }

    pub fn entry(&self, path: &Path) -> Option<Node> {
        self.nodes.get(path).map(|node| node.clone())
    }

    pub fn add_local(
        &self,
        path: &Path,
        local: fsync::Metadata,
    ) -> std::result::Result<(), fsync::Metadata> {
        let node = self.nodes.get_mut(path);
        if node.is_none() {
            return Err(local);
        }
        let mut node = node.unwrap();
        node.add_local(local);
        Ok(())
    }

    pub fn add_remote(
        &self,
        path: &Path,
        remote: fsync::Metadata,
    ) -> std::result::Result<(), fsync::Metadata> {
        let node = self.nodes.get_mut(path);
        if node.is_none() {
            return Err(remote);
        }
        let mut node = node.unwrap();
        node.add_remote(remote);
        Ok(())
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
            Entry::Both { .. } => "B",
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
    nodes: &'a DashMap<PathBuf, Node>,
}

impl<'a, L, R> DiffTreeBuild<'a, L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    fn both(
        &self,
        local: fsync::Metadata,
        remote: fsync::Metadata,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
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
                            joinvec.push(self.both(loc.clone(), rem.clone()));
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

            assert_eq!(local.path(), remote.path());
            let path = local.path().to_owned();
            let entry = Entry::Both { local, remote };

            let node = Node::new(entry, children);
            self.nodes.insert(path, node);

            Ok(())
        })
    }

    fn local(&self, entry: fsync::Metadata) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            let mut child_names = Vec::new();

            if entry.is_dir() {
                let mut joinvec = Vec::new();
                let children = self.local.dir_entries(entry.path().to_owned());
                tokio::pin!(children);

                while let Some(child) = children.next().await {
                    let child = child?;
                    child_names.push(child.name().to_owned());
                    joinvec.push(self.local(child));
                }
                future::try_join_all(joinvec).await?;
            }

            let path = entry.path().to_owned();
            let entry = Entry::Local(entry);
            let node = Node::new(entry, child_names);
            self.nodes.insert(path, node);
            Ok(())
        })
    }

    fn remote(&self, entry: fsync::Metadata) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            let mut child_names = Vec::new();

            if entry.is_dir() {
                let mut joinvec = Vec::new();
                let children = self.remote.dir_entries(entry.path().to_owned());
                tokio::pin!(children);

                while let Some(child) = children.next().await {
                    let child = child?;
                    child_names.push(child.name().to_owned());
                    joinvec.push(self.remote(child));
                }
                future::try_join_all(joinvec).await?;
            }

            let path = entry.path().to_owned();
            let entry = Entry::Remote(entry);
            let node = Node::new(entry, child_names);
            self.nodes.insert(path, node);
            Ok(())
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
    let path = entry.path().to_owned();
    let children = storage.dir_entries(path);
    let mut children = children.try_collect::<Vec<_>>().await?;

    children.sort_unstable_by(|a, b| a.name().cmp(b.name()));

    Ok(children)
}
