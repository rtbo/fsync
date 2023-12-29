use camino::Utf8Path;
use serde::{Deserialize, Serialize};

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
    pub fn new(entry: Entry, children: Vec<String>) -> Self {
        Self { entry, children }
    }

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
