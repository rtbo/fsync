use camino::{Utf8Path, Utf8PathBuf};
use dashmap::DashMap;
use fsync::EntryType;

use crate::cache::Cache;

pub enum TreeEntry {
    Local {
        typ: EntryType,
    },
    Remote {
        id: String,
        typ: EntryType,
    },
    Both {
        local_typ: EntryType,
        remote_id: String,
        remote_typ: EntryType,
    },
}

struct TreeNode {
    entry: TreeEntry,
    children: Vec<String>,
}

pub struct DiffTree {
    nodes: DashMap<Utf8PathBuf, TreeNode>,
}

impl DiffTree {
    pub fn from_cache(local: &Cache, remote: &Cache) -> Self {
        let nodes = DashMap::new();
        let build = DiffTreeBuild {
            local,
            remote,
            nodes: &nodes,
        };
        build.both(None);
        Self { nodes }
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

struct DiffTreeBuild<'a> {
    local: &'a Cache,
    remote: &'a Cache,
    nodes: &'a DashMap<Utf8PathBuf, TreeNode>,
}

impl<'a> DiffTreeBuild<'a> {
    fn both(&self, path: Option<&Utf8Path>) {
        let loc_cache_entry = self.local.entry(path).unwrap();
        let rem_cache_entry = self.remote.entry(path).unwrap();

        let mut iloc = 0usize;
        let mut irem = 0usize;

        let mut children = Vec::new();

        while iloc < loc_cache_entry.children.len() && irem < rem_cache_entry.children.len() {
            let loc = &loc_cache_entry.children[iloc];
            let rem = &rem_cache_entry.children[irem];
            if loc == rem {
                let path = join_path(path, loc);
                let loc_entry = self.local.entry(Some(&path)).unwrap();
                let rem_entry = self.remote.entry(Some(&path)).unwrap();
                match (loc_entry.entry.is_dir(), rem_entry.entry.is_dir()) {
                    (true, true) => {
                        self.both(Some(&path));
                    }
                    (true, false) => {
                        self.local(path.clone());
                    }
                    (false, true) => {
                        self.remote(path.clone());
                    }
                    (false, false) => {
                        self.both(Some(&path));
                    }
                }
                children.push(loc.clone());
                iloc += 1;
                irem += 1;
            } else if loc < rem {
                self.local(join_path(path, loc));
                children.push(loc.clone());
                iloc += 1;
                iloc += 1;
            } else {
                self.remote(join_path(path, rem));
                children.push(rem.clone());
                irem += 1;
            };
        }

        for child_name in &loc_cache_entry.children[iloc..] {
            self.local(join_path(path, child_name));
            children.push(child_name.clone());
        }
        for child_name in &rem_cache_entry.children[irem..] {
            self.remote(join_path(path, child_name));
            children.push(child_name.clone());
        }

        let path = path
            .map(|p| p.to_owned())
            .unwrap_or_else(|| Utf8PathBuf::new());

        let entry = TreeEntry::Both {
            local_typ: loc_cache_entry.entry.typ().clone(),
            remote_id: rem_cache_entry.entry.id().to_string(),
            remote_typ: rem_cache_entry.entry.typ().clone(),
        };
        let node = TreeNode { entry, children };
        self.nodes.insert(path, node);
    }

    fn local(&self, path: Utf8PathBuf) {
        let cache_entry = self.local.entry(Some(&path)).unwrap();

        let children = cache_entry.children.clone();

        for child_name in &children {
            self.local(join_path(Some(&path), child_name));
        }

        let entry = TreeEntry::Local {
            typ: cache_entry.entry.typ().clone(),
        };
        let node = TreeNode { entry, children };
        self.nodes.insert(path, node);
    }

    fn remote(&self, path: Utf8PathBuf) {
        let cache_entry = self.remote.entry(Some(&path)).unwrap();

        let children = cache_entry.children.clone();

        for child_name in &children {
            self.remote(join_path(Some(&path), child_name));
        }

        let entry = TreeEntry::Remote {
            id: cache_entry.entry.id().to_string(),
            typ: cache_entry.entry.typ().clone(),
        };
        let node = TreeNode { entry, children };
        self.nodes.insert(path, node);
    }
}

fn join_path(path: Option<&Utf8Path>, child_name: &str) -> Utf8PathBuf {
    path.map(|p| p.join(child_name))
        .unwrap_or_else(|| Utf8PathBuf::from(child_name))
}
