use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

use crate::{
    path::{Path, PathBuf},
    stat,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum Metadata {
    Directory {
        path: PathBuf,
        stat: Option<stat::Dir>,
    },
    Regular {
        path: PathBuf,
        size: u64,
        #[type_def(type_of = "i64")]
        #[serde(with = "ms_since_epoch")]
        mtime: DateTime<Utc>,
    },
}

/// Serialize a `DateTime` in milliseconds since the Unix epoch (to fit with Javascript representation).
/// The milliseconds are rounded down to the nearest second however.
/// This is because some provider do not provide millisecond granularity in the timestamps.
mod ms_since_epoch {
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(1000 * date.timestamp() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        let naive = chrono::NaiveDateTime::from_timestamp_millis(millis as i64).unwrap();
        let utc = Utc;
        Ok(utc.from_utc_datetime(&naive))
    }
}

impl Metadata {
    pub fn root() -> Self {
        Self::Directory {
            path: PathBuf::root(),
            stat: None,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Directory { path, .. } => path,
            Self::Regular { path, .. } => path,
        }
    }

    pub fn name(&self) -> &str {
        self.path().file_name().unwrap_or("")
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Self::Directory { .. })
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::Regular { .. })
    }

    pub fn size(&self) -> Option<u64> {
        match self {
            Self::Regular { size, .. } => Some(*size),
            _ => None,
        }
    }

    pub fn mtime(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Regular { mtime, .. } => Some(*mtime),
            _ => None,
        }
    }

    pub fn has_stat(&self) -> bool {
        matches!(self, Self::Directory { stat, .. } if stat.is_some())
    }

    pub fn children_stat(&self) -> Option<stat::Dir> {
        match self {
            Self::Directory { stat, .. } => *stat,
            _ => None,
        }
    }

    pub fn stat(&self) -> Option<stat::Dir> {
        match self {
            Self::Directory { stat, .. } => stat.map(|s| s.with_dirs(s.dirs + 1)),
            Self::Regular { size, .. } => Some(stat::Dir {
                data: *size as _,
                dirs: 0,
                files: 1,
            }),
        }
    }

    pub fn add_stat(&mut self, added: &stat::Dir) {
        match self {
            Metadata::Directory {
                stat: Some(stat), ..
            } => *stat += *added,
            Metadata::Directory { stat, .. } => *stat = Some(*added),
            _ if added.is_null() => (),
            _ => panic!("Not a directory"),
        }
    }
}

pub mod tree {
    use std::mem;

    use serde::{Deserialize, Serialize};
    use typescript_type_def::TypeDef;

    use crate::{path::Path, stat, Conflict, StorageLoc};

    #[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
    #[serde(rename_all = "camelCase")]
    pub enum Entry {
        Local(super::Metadata),
        Remote(super::Metadata),
        Sync {
            local: super::Metadata,
            remote: super::Metadata,
            /// Conflict for this very entry
            conflict: Option<Conflict>,
        },
    }

    impl Entry {
        pub fn new_sync(local: super::Metadata, remote: super::Metadata) -> Self {
            let conflict = Conflict::check(&local, &remote);
            Self::Sync {
                local,
                remote,
                conflict,
            }
        }

        pub fn new_at(metadata: super::Metadata, loc: StorageLoc) -> Self {
            match loc {
                StorageLoc::Local => Self::Local(metadata),
                StorageLoc::Remote => Self::Remote(metadata),
            }
        }

        pub fn root() -> Self {
            Self::Sync {
                local: super::Metadata::root(),
                remote: super::Metadata::root(),
                conflict: None,
            }
        }

        pub fn path(&self) -> &Path {
            match self {
                Entry::Sync { local, remote, .. } => {
                    debug_assert_eq!(local.path(), remote.path());
                    local.path()
                }
                Entry::Local(entry) => entry.path(),
                Entry::Remote(entry) => entry.path(),
            }
        }

        pub fn name(&self) -> Option<&str> {
            self.path().file_name()
        }

        pub fn into_metadata(self, loc: StorageLoc) -> Option<super::Metadata> {
            match loc {
                StorageLoc::Local => self.into_local_metadata(),
                StorageLoc::Remote => self.into_remote_metadata(),
            }
        }

        pub fn into_local_metadata(self) -> Option<super::Metadata> {
            match self {
                Self::Local(metadata) => Some(metadata),
                Self::Remote(..) => None,
                Self::Sync { local, .. } => Some(local),
            }
        }

        pub fn into_remote_metadata(self) -> Option<super::Metadata> {
            match self {
                Self::Local(..) => None,
                Self::Remote(metadata) => Some(metadata),
                Self::Sync { remote, .. } => Some(remote),
            }
        }

        pub fn is_local_only(&self) -> bool {
            matches!(self, Entry::Local(..))
        }

        pub fn is_remote_only(&self) -> bool {
            matches!(self, Entry::Remote(..))
        }

        pub fn is_sync(&self) -> bool {
            matches!(self, Entry::Sync { .. })
        }

        pub fn is_local_dir(&self) -> bool {
            matches!(self, Entry::Local(md) if md.is_dir())
        }

        pub fn is_remote_dir(&self) -> bool {
            matches!(self, Entry::Remote(md) if md.is_dir())
        }

        pub fn is_sync_dir(&self) -> bool {
            matches!(self, Entry::Sync{local, remote, ..} if local.is_dir() && remote.is_dir())
        }

        pub fn is_safe_dir(&self) -> bool {
            match self {
                Self::Local(md) if md.is_dir() => true,
                Self::Remote(md) if md.is_dir() => true,
                Self::Sync { local, remote, .. } if local.is_dir() && remote.is_dir() => true,
                _ => false,
            }
        }

        pub fn conflict(&self) -> Option<Conflict> {
            match self {
                Self::Sync { conflict, .. } => *conflict,
                _ => None,
            }
        }

        pub fn is_conflict(&self) -> bool {
            matches!(
                self,
                Self::Sync {
                    conflict: Some(_),
                    ..
                }
            )
        }

        pub fn is_at_loc(&self, loc: StorageLoc) -> bool {
            match loc {
                StorageLoc::Local => self.is_local_only() || self.is_sync(),
                StorageLoc::Remote => self.is_remote_only() || self.is_sync(),
            }
        }

        pub fn is_only_at_loc(&self, loc: StorageLoc) -> bool {
            match loc {
                StorageLoc::Local => self.is_local_only(),
                StorageLoc::Remote => self.is_remote_only(),
            }
        }

        pub fn stat_by_loc(&self, loc: StorageLoc) -> Option<stat::Dir> {
            match loc {
                StorageLoc::Local => self.local_stat(),
                StorageLoc::Remote => self.remote_stat(),
            }
        }

        pub fn local_stat(&self) -> Option<stat::Dir> {
            match self {
                Self::Local(local) => local.stat(),
                Self::Sync { local, .. } => local.stat(),
                _ => None,
            }
        }

        pub fn remote_stat(&self) -> Option<stat::Dir> {
            match self {
                Self::Remote(remote) => remote.stat(),
                Self::Sync { remote, .. } => remote.stat(),
                _ => None,
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
    #[serde(rename_all = "camelCase")]
    pub struct EntryNode {
        entry: Entry,
        children: Vec<String>,
        children_node_stat: stat::Node,
    }

    impl EntryNode {
        pub fn new(entry: Entry, children: Vec<String>, children_stat: stat::Tree) -> Self {
            let mut entry = entry;
            match &mut entry {
                Entry::Local(local) => {
                    debug_assert!(
                        !local.has_stat(),
                        "Entry should not have stat in EntryNode::new"
                    );
                    debug_assert!(
                        children_stat.remote.is_null(),
                        "Remote stat should be null for local entry"
                    );
                    debug_assert!(
                        local.is_dir() || children_stat.local.is_null(),
                        "Stat should be null for non-dir entry"
                    );
                    local.add_stat(&children_stat.local);
                }
                Entry::Remote(remote) => {
                    debug_assert!(
                        !remote.has_stat(),
                        "Entry should not have stat in EntryNode::new"
                    );
                    debug_assert!(
                        children_stat.local.is_null(),
                        "Local stat should be null for remote entry"
                    );
                    debug_assert!(
                        remote.is_dir() || children_stat.remote.is_null(),
                        "Stat should be null for non-dir entry"
                    );
                    remote.add_stat(&children_stat.remote);
                }
                Entry::Sync { local, remote, .. } => {
                    debug_assert!(
                        !local.has_stat(),
                        "Entry should not have stat in EntryNode::new"
                    );
                    debug_assert!(
                        !remote.has_stat(),
                        "Entry should not have stat in EntryNode::new"
                    );
                    debug_assert!(
                        local.is_dir() || children_stat.local.is_null(),
                        "Stat should be null for non-dir entry"
                    );
                    debug_assert!(
                        remote.is_dir() || children_stat.remote.is_null(),
                        "Stat should be null for non-dir entry"
                    );
                    local.add_stat(&children_stat.local);
                    remote.add_stat(&children_stat.remote);
                }
            }

            Self {
                entry,
                children,
                children_node_stat: children_stat.node,
            }
        }

        pub fn without_children(self) -> Self {
            Self {
                entry: self.entry,
                children: Vec::new(),
                children_node_stat: stat::Node::null(),
            }
        }

        pub fn entry(&self) -> &Entry {
            &self.entry
        }

        pub fn entry_mut(&mut self) -> &mut Entry {
            &mut self.entry
        }

        pub fn op_entry<F: FnOnce(Entry) -> Entry>(&mut self, op: F) {
            let invalid: Entry = unsafe { mem::MaybeUninit::zeroed().assume_init() };
            let valid = mem::replace(&mut self.entry, invalid);
            self.entry = op(valid);
        }

        pub fn into_entry(self) -> Entry {
            self.entry
        }

        pub fn into_parts(self) -> (Entry, Vec<String>, stat::Node) {
            (self.entry, self.children, self.children_node_stat)
        }

        pub fn children(&self) -> &[String] {
            &self.children
        }

        pub fn path(&self) -> &Path {
            self.entry.path()
        }

        pub fn name(&self) -> Option<&str> {
            self.path().file_name()
        }

        pub fn is_local_only(&self) -> bool {
            self.entry.is_local_only()
        }

        pub fn is_remote_only(&self) -> bool {
            self.entry.is_remote_only()
        }

        pub fn is_at_loc(&self, loc: StorageLoc) -> bool {
            self.entry.is_at_loc(loc)
        }

        pub fn is_only_at_loc(&self, loc: StorageLoc) -> bool {
            self.entry.is_only_at_loc(loc)
        }

        pub fn is_sync(&self) -> bool {
            self.entry.is_sync()
        }

        pub fn children_conflicts(&self) -> u32 {
            self.children_node_stat.conflicts as _
        }

        pub fn children_have_conflicts(&self) -> bool {
            self.children_conflicts() > 0
        }

        /// Get the stat for this node.
        ///
        /// # Panics
        /// Panics if either the local or remote stat (as relevant) is invalid.
        pub fn stats(&self) -> stat::Tree {
            match self.entry() {
                Entry::Sync {
                    local,
                    remote,
                    conflict,
                } => {
                    let added = stat::Node {
                        nodes: 1,
                        sync: 1,
                        conflicts: if conflict.is_some() { 1 } else { 0 },
                    };
                    stat::Tree {
                        local: local.stat().expect("local stat should be valid"),
                        remote: remote.stat().expect("remote stat should be valid"),
                        node: self.children_node_stat + added,
                    }
                }
                Entry::Local(entry) => {
                    let added = stat::Node {
                        nodes: 1,
                        sync: 0,
                        conflicts: 0,
                    };
                    stat::Tree {
                        local: entry.stat().expect("local stat should be valid"),
                        remote: stat::Dir::null(),
                        node: self.children_node_stat + added,
                    }
                }
                Entry::Remote(entry) => {
                    let added = stat::Node {
                        nodes: 1,
                        sync: 0,
                        conflicts: 0,
                    };
                    stat::Tree {
                        local: stat::Dir::null(),
                        remote: entry.stat().expect("remote stat should be valid"),
                        node: self.children_node_stat + added,
                    }
                }
            }
        }

        pub fn add_stat(&mut self, added: &stat::Tree) {
            match &mut self.entry {
                Entry::Local(local) => {
                    local.add_stat(&added.local);
                }
                Entry::Remote(remote) => {
                    remote.add_stat(&added.remote);
                }
                Entry::Sync { local, remote, .. } => {
                    local.add_stat(&added.local);
                    remote.add_stat(&added.remote);
                }
            }
            self.children_node_stat += added.node;
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionMethod {
    ReplaceOlderByNewer,
    ReplaceNewerByOlder,
    ReplaceLocalByRemote,
    ReplaceRemoteByLocal,
    DeleteOlder,
    DeleteNewer,
    DeleteLocal,
    DeleteRemote,
    CreateLocalCopy,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum DeletionMethod {
    /// Will delete local files and folders only if they are synced with remote.
    /// Files with conflict will be deleted as well.
    LocalIfSync,
    /// Will delete remote files and folders only if they are synced locally.
    /// Files with conflict will be deleted as well.
    RemoteIfSync,
    /// Will delete local files and folders only if they are synced with remote.
    /// Files with conflict won't be deleted.
    LocalIfSyncNoConflict,
    /// Will delete remote files and folders only if they are synced locally.
    /// Files with conflict won't be deleted.
    RemoteIfSyncNoConflict,
    /// Will delete all local files and folders.
    Local,
    /// Will delete all remote files and folders.
    Remote,
    /// Will delete both local and remote files and folders, losing all data.
    All,
}

impl DeletionMethod {
    pub const fn is_local(&self) -> bool {
        matches!(
            self,
            DeletionMethod::Local
                | DeletionMethod::LocalIfSync
                | DeletionMethod::LocalIfSyncNoConflict
        )
    }

    pub const fn is_remote(&self) -> bool {
        matches!(
            self,
            DeletionMethod::Remote
                | DeletionMethod::RemoteIfSync
                | DeletionMethod::RemoteIfSyncNoConflict
        )
    }

    pub const fn no_conflict(&self) -> bool {
        matches!(
            self,
            DeletionMethod::LocalIfSyncNoConflict | DeletionMethod::RemoteIfSyncNoConflict
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Sync(PathBuf),
    Resolve(PathBuf, ResolutionMethod),
    Delete(PathBuf, DeletionMethod),

    SyncDeep(PathBuf),
    ResolveDeep(PathBuf, ResolutionMethod),
    DeleteDeep(PathBuf, DeletionMethod),
}

impl Operation {
    pub fn path(&self) -> &Path {
        match self {
            Operation::Sync(path) => path,
            Operation::Resolve(path, _) => path,
            Operation::Delete(path, _) => path,

            Operation::SyncDeep(path) => path,
            Operation::ResolveDeep(path, _) => path,
            Operation::DeleteDeep(path, _) => path,
        }
    }

    pub const fn is_deep(&self) -> bool {
        matches!(
            self,
            Operation::SyncDeep(..) | Operation::ResolveDeep(..) | Operation::DeleteDeep(..)
        )
    }

    pub fn not_deep(self) -> Self {
        match self {
            Operation::SyncDeep(path) => Operation::Sync(path),
            Operation::ResolveDeep(path, method) => Operation::Resolve(path, method),
            Operation::DeleteDeep(path, method) => Operation::Delete(path, method),
            op => panic!("Not a deep operation: {op:?}"),
        }
    }

    pub fn with_path(&self, path: PathBuf) -> Self {
        match self {
            Operation::Sync(_) => Operation::Sync(path),
            Operation::Resolve(_, method) => Operation::Resolve(path, *method),
            Operation::Delete(_, method) => Operation::Delete(path, *method),

            Operation::SyncDeep(_) => Operation::SyncDeep(path),
            Operation::ResolveDeep(_, method) => Operation::ResolveDeep(path, *method),
            Operation::DeleteDeep(_, method) => Operation::DeleteDeep(path, *method),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum Progress {
    Init,
    OAuth2Browse(String),
    OAuth2Exchange,
    OAuth2Refresh,
    Progress { progress: u64, total: u64 },
    Compound,
    Done,
    Err(crate::Error),
}

impl Progress {
    pub fn is_done(&self) -> bool {
        matches!(self, Self::Done)
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::Init
    }
}

#[tarpc::service]
pub trait Fsync {
    async fn conflicts(first: Option<PathBuf>, max_len: u32) -> crate::Result<Vec<tree::Entry>>;
    async fn entry_node(path: PathBuf) -> crate::Result<Option<tree::EntryNode>>;
    async fn operate(operation: Operation) -> crate::Result<Progress>;
    /// Provide the progress of the operation on the given path.
    async fn progress(path: PathBuf) -> crate::Result<Option<Progress>>;
    /// Provide the progress of all operations of the given path and its descendants.
    async fn progresses(path: PathBuf) -> crate::Result<Vec<(PathBuf, Progress)>>;
}
