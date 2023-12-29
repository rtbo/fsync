use std::cmp::Ordering;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use dashmap::DashMap;
use futures::{
    future::{self, BoxFuture},
    StreamExt, TryStreamExt,
};
use serde::{Deserialize, Serialize};

use crate::{Result, Storage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Entry {
    Local(crate::Entry),
    Remote(crate::Entry),
    Both {
        local: crate::Entry,
        remote: crate::Entry,
    },
}

impl Entry {
    fn with_remote(self, remote: crate::Entry) -> Self {
        match self {
            Entry::Local(local) => Entry::Both { local, remote },
            Entry::Remote(..) => Entry::Remote(remote),
            Entry::Both { local, .. } => Entry::Both { local, remote },
        }
    }

    fn with_local(self, local: crate::Entry) -> Self {
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
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn children(&self) -> &[String] {
        &self.children
    }

    pub fn path(&self) -> &Utf8Path {
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

    pub fn add_local(&mut self, local: crate::Entry) {
        use std::mem;
        let invalid: Entry = unsafe { mem::MaybeUninit::zeroed().assume_init() };
        let valid = mem::replace(&mut self.entry, invalid);
        self.entry = valid.with_local(local);
    }

    pub fn add_remote(&mut self, remote: crate::Entry) {
        use std::mem;
        let invalid: Entry = unsafe { mem::MaybeUninit::zeroed().assume_init() };
        let valid = mem::replace(&mut self.entry, invalid);
        self.entry = valid.with_remote(remote);
    }
}

#[derive(Debug)]
pub struct DiffTree {
    nodes: Arc<DashMap<Utf8PathBuf, Node>>,
}

impl DiffTree {
    pub async fn from_cache<L, R>(local: Arc<L>, remote: Arc<R>) -> Result<Self>
    where
        L: Storage,
        R: Storage,
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

    pub fn entry(&self, path: Option<&Utf8Path>) -> Option<Node> {
        let key = path.unwrap_or(Utf8Path::new(""));
        self.nodes.get(key).map(|node| node.clone())
    }

    pub fn add_local(
        &self,
        path: &Utf8Path,
        local: crate::Entry,
    ) -> std::result::Result<(), crate::Entry> {
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
        path: &Utf8Path,
        remote: crate::Entry,
    ) -> std::result::Result<(), crate::Entry> {
        let node = self.nodes.get_mut(path);
        if node.is_none() {
            return Err(remote);
        }
        let mut node = node.unwrap();
        node.add_remote(remote);
        Ok(())
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
            Entry::Both { .. } => "B",
            Entry::Local { .. } => "L",
            Entry::Remote { .. } => "R",
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
    nodes: Arc<DashMap<Utf8PathBuf, Node>>,
}

impl<L, R> DiffTreeBuild<L, R>
where
    L: Storage,
    R: Storage,
{
    fn both(&self, both: Option<(crate::Entry, crate::Entry)>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let loc_entry = both.as_ref().map(|b| &b.0);
            let loc_children = entry_children_sorted(&*self.local, loc_entry);

            let rem_entry = both.as_ref().map(|b| &b.1);
            let rem_children = entry_children_sorted(&*self.remote, rem_entry);

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
                (path, Entry::Both { local, remote })
            } else {
                (
                    Utf8PathBuf::default(),
                    Entry::Both {
                        local: crate::Entry::default(),
                        remote: crate::Entry::default(),
                    },
                )
            };

            let node = Node { entry, children };
            self.nodes.insert(path, node);

            Ok(())
        })
    }

    fn local(&self, entry: crate::Entry) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut child_names = Vec::new();

            if entry.is_dir() {
                let mut joinvec = Vec::new();
                let children = self.local.dir_entries(Some(entry.path_id()));
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
            let node = Node {
                entry,
                children: child_names,
            };
            self.nodes.insert(path, node);
            Ok(())
        })
    }

    fn remote(&self, entry: crate::Entry) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut child_names = Vec::new();

            if entry.is_dir() {
                let mut joinvec = Vec::new();
                let children = self.remote.dir_entries(Some(entry.path_id()));
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
            let node = Node {
                entry,
                children: child_names,
            };
            self.nodes.insert(path, node);
            Ok(())
        })
    }
}

async fn entry_children_sorted<S>(
    storage: &S,
    entry: Option<&crate::Entry>,
) -> Result<Vec<crate::Entry>>
where
    S: Storage,
{
    if let Some(entry) = entry {
        if !entry.is_dir() {
            return Ok(vec![]);
        }
    }
    let path_id = entry.map(|e| e.path_id());
    let children = storage.dir_entries(path_id);
    let mut children = children.try_collect::<Vec<_>>().await?;

    children.sort_unstable_by(|a, b| a.name().cmp(b.name()));

    Ok(children)
}