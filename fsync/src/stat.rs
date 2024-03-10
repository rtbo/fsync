use std::ops;

use serde::{Deserialize, Serialize};

use crate::StorageLoc;

/// Stats for a directory.
/// This is recursive stats for all children of a directory,
/// including grand-children and so forth
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dir {
    /// The data in the directory, in bytes
    pub data: i64,
    /// The number of directory entries in this directory
    pub dirs: i32,
    /// The number of file entries in this directory
    pub files: i32,
}

impl Dir {
    pub fn null() -> Self {
        Self {
            data: 0,
            dirs: 0,
            files: 0,
        }
    }

    pub fn is_null(&self) -> bool {
        self.data == 0 && self.dirs == 0 && self.files == 0
    }

    pub fn is_positive(&self) -> bool {
        self.data >= 0 && self.dirs >= 0 && self.files >= 0
    }

    pub fn entries(&self) -> i32 {
        self.dirs + self.files
    }

    pub fn with_data(self, data: i64) -> Self {
        Self { data, ..self }
    }

    pub fn with_dirs(self, dirs: i32) -> Self {
        Self { dirs, ..self }
    }

    pub fn with_files(self, files: i32) -> Self {
        Self { files, ..self }
    }
}

impl ops::Add for Dir {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            data: self.data + rhs.data,
            dirs: self.dirs + rhs.dirs,
            files: self.files + rhs.files,
        }
    }
}

impl ops::AddAssign for Dir {
    fn add_assign(&mut self, rhs: Self) {
        self.data += rhs.data;
        self.dirs += rhs.dirs;
        self.files += rhs.files;
    }
}

impl ops::Sub for Dir {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            data: self.data - rhs.data,
            dirs: self.dirs - rhs.dirs,
            files: self.files - rhs.files,
        }
    }
}

impl ops::SubAssign for Dir {
    fn sub_assign(&mut self, rhs: Self) {
        self.data -= rhs.data;
        self.dirs -= rhs.dirs;
        self.files -= rhs.files;
    }
}

/// Stats for a Node in the tree structure.
/// That is, the stats for both local and remote files and directories
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub nodes: i32,
    pub sync: i32,
    pub conflicts: i32,
}

impl Node {
    pub fn null() -> Self {
        Self {
            nodes: 0,
            sync: 0,
            conflicts: 0,
        }
    }

    pub fn is_null(&self) -> bool {
        self.nodes == 0 && self.sync == 0 && self.conflicts == 0
    }

    pub fn is_positive(&self) -> bool {
        self.nodes >= 0 && self.sync >= 0 && self.conflicts >= 0
    }

    pub fn entries(&self) -> i32 {
        self.sync + self.conflicts
    }

    pub fn with_nodes(self, nodes: i32) -> Self {
        Self { nodes, ..self }
    }

    pub fn with_sync(self, sync: i32) -> Self {
        Self { sync, ..self }
    }

    pub fn with_conflicts(self, conflicts: i32) -> Self {
        Self { conflicts, ..self }
    }
}

impl ops::Add for Node {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            nodes: self.nodes + rhs.nodes,
            sync: self.sync + rhs.sync,
            conflicts: self.conflicts + rhs.conflicts,
        }
    }
}

impl ops::AddAssign for Node {
    fn add_assign(&mut self, rhs: Self) {
        self.nodes += rhs.nodes;
        self.sync += rhs.sync;
        self.conflicts += rhs.conflicts;
    }
}

impl ops::Sub for Node {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            nodes: self.nodes - rhs.nodes,
            sync: self.sync - rhs.sync,
            conflicts: self.conflicts - rhs.conflicts,
        }
    }
}

impl ops::SubAssign for Node {
    fn sub_assign(&mut self, rhs: Self) {
        self.nodes -= rhs.nodes;
        self.sync -= rhs.sync;
        self.conflicts -= rhs.conflicts;
    }
}

/// Stats for the whole diff tree structure.
/// That is, the stats for both local and remote files and directories
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Tree {
    pub local: Dir,
    pub remote: Dir,
    pub node: Node,
}

impl Tree {
    pub fn null() -> Self {
        Tree {
            local: Dir::null(),
            remote: Dir::null(),
            node: Node::null(),
        }
    }

    pub fn is_null(&self) -> bool {
        self.local.is_null() && self.remote.is_null() && self.node.is_null()
    }

    pub fn is_positive(&self) -> bool {
        self.local.is_positive() && self.remote.is_positive() && self.node.is_positive()
    }

    pub fn by_loc(&self, loc: StorageLoc) -> &Dir {
        match loc {
            StorageLoc::Local => &self.local,
            StorageLoc::Remote => &self.remote,
        }
    }
}

impl ops::Add for Tree {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            local: self.local + rhs.local,
            remote: self.remote + rhs.remote,
            node: self.node + rhs.node,
        }
    }
}

impl ops::AddAssign for Tree {
    fn add_assign(&mut self, rhs: Self) {
        self.local += rhs.local;
        self.remote += rhs.remote;
        self.node += rhs.node;
    }
}

impl ops::Sub for Tree {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            local: self.local - rhs.local,
            remote: self.remote - rhs.remote,
            node: self.node - rhs.node,
        }
    }
}

impl ops::SubAssign for Tree {
    fn sub_assign(&mut self, rhs: Self) {
        self.local -= rhs.local;
        self.remote -= rhs.remote;
        self.node -= rhs.node;
    }
}
