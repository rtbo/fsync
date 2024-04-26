use std::time::{Duration, SystemTime};

use fsync::{
    path::{Component, FsPath, Path, PathBuf},
    stat,
};
use fsyncd::storage::cache::{CachePersist, CacheStorage};
use futures::future::BoxFuture;
use tokio::io::AsyncWriteExt;

use crate::stubs;

mod build {
    #[derive(Debug, Copy, Clone)]
    pub enum Entry {
        Dir {
            /// Name of the directory
            name: &'static str,
            /// Entries of the directory
            entries: &'static [Entry],
        },
        File {
            /// Name of the file
            name: &'static str,
            /// Content of the file
            content: &'static str,
            /// Age of the file in seconds
            age: Option<u32>,
        },
    }

    #[rustfmt::skip]
    pub const LOCAL: &[Entry] = &[
        Entry::File{name: "only-local.txt", content: "/only-local.txt", age: None},
        Entry::File{name: "both.txt", content: "/both.txt", age: None},
        Entry::File{name: "conflict.txt", content: "/conflict.txt - local", age: None},
        Entry::Dir{name: "only-local", entries: &[
            Entry::File{name: "file1.txt", content: "/only-local/file1.txt", age: None},
            Entry::File{name: "file2.txt", content: "/only-local/file2.txt", age: None},
            Entry::Dir{name: "deep", entries: &[
                Entry::File{name: "file1.txt", content: "/only-local/deep/file1.txt", age: None},
                Entry::File{name: "file2.txt", content: "/only-local/deep/file2.txt", age: None},
            ]},
        ]},
        Entry::Dir{name: "both", entries: &[
            Entry::File{name: "both.txt", content: "/both/both.txt", age: None},
            Entry::File{name: "conflict.txt", content: "/both/conflict.txt - local", age: None},
            Entry::File{name: "only-local.txt", content: "/both/only-local.txt", age: None},
            Entry::Dir{name: "deep", entries: &[
                Entry::File{name: "file1.txt", content: "/both/deep/file1.txt", age: None},
                Entry::File{name: "file2.txt", content: "/both/deep/file2.txt", age: None},
            ]},
        ]},
    ];

    #[rustfmt::skip]
    pub const REMOTE: &[Entry] = &[
        Entry::File{name: "only-remote.txt", content: "/only-remote.txt", age: None},
        Entry::File{name: "both.txt", content: "/both.txt", age: None},
        Entry::File{name: "conflict.txt", content: "/conflict.txt - remote", age: None},
        Entry::Dir{name: "only-remote", entries: &[
            Entry::File{name: "file1.txt", content: "/only-remote/file1.txt", age: None},
            Entry::File{name: "file2.txt", content: "/only-remote/file2.txt", age: None},
            Entry::Dir{name: "deep", entries: &[
                Entry::File{name: "file1.txt", content: "/only-remote/deep/file1.txt", age: None},
                Entry::File{name: "file2.txt", content: "/only-remote/deep/file2.txt", age: None},
            ]},
        ]},
        Entry::Dir{name: "both", entries: &[
            Entry::File{name: "both.txt", content: "/both/both.txt", age: None},
            Entry::File{name: "conflict.txt", content: "/both/conflict.txt - remote", age: None},
            Entry::File{name: "only-remote.txt", content: "/both/only-remote.txt", age: None},
            Entry::Dir{name: "deep", entries: &[
                Entry::File{name: "file1.txt", content: "/both/deep/file1.txt", age: None},
                Entry::File{name: "file2.txt", content: "/both/deep/file2.txt", age: None},
            ]},
        ]},
    ];
}

pub const DEFAULT_NODE_STAT: stat::Node = stat::Node {
    nodes: 25,    // 24 + root
    sync: 9,      // 8 + root
    conflicts: 2, // 2 'conflict.txt' (have different content)
};

#[derive(Debug, Clone)]
pub enum Entry {
    Dir {
        name: String,
        entries: Vec<Entry>,
    },
    File {
        name: String,
        content: String,
        age: Option<u32>,
    },
}

impl Entry {
    pub fn name(&self) -> &str {
        match self {
            Entry::Dir { name, .. } => name,
            Entry::File { name, .. } => name,
        }
    }

    pub fn content(&self) -> Option<&str> {
        match self {
            Entry::File { content, .. } => Some(content.as_str()),
            _ => None,
        }
    }

    pub fn age(&self) -> Option<u32> {
        match self {
            Entry::File { age, .. } => *age,
            _ => None,
        }
    }
}

impl From<build::Entry> for Entry {
    fn from(e: build::Entry) -> Self {
        match e {
            build::Entry::Dir { name, entries } => Entry::Dir {
                name: name.into(),
                entries: entries.iter().map(|e| (*e).into()).collect(),
            },
            build::Entry::File { name, content, age } => Entry::File {
                name: name.into(),
                content: content.into(),
                age,
            },
        }
    }
}

fn resolve_entry<'a>(entries: &'a mut [Entry], path: &Path) -> Option<&'a mut Entry> {
    fn inner<'a>(ent: &'a mut [Entry], p: &Path) -> Option<&'a mut Entry> {
        let mut comps = p.components();
        match comps.next() {
            Some(Component::Normal(name)) => {
                let entry = ent.iter_mut().find(|e| e.name() == name);
                if comps.clone().next().is_none() {
                    return entry;
                }
                match entry {
                    Some(Entry::Dir { entries, .. }) => inner(entries, comps.as_path()),
                    _ => None,
                }
            }
            None => None,
            _ => unreachable!("Invalid path"),
        }
    }

    assert!(path.is_absolute(), "resolve_entry expects absolute path");
    let path = path.without_root();
    inner(entries, &path)
}

#[test]
fn test_resolve_entry() {
    let mut entries = build::LOCAL.iter().map(|e| (*e).into()).collect::<Vec<_>>();
    {
        let resolved =
            resolve_entry(&mut entries, Path::new("/only-local.txt")).expect("Expected an entry");
        assert_eq!("only-local.txt", resolved.name());
        assert_eq!(Some("/only-local.txt"), resolved.content());
    }

    {
        let resolved = resolve_entry(&mut entries, Path::new("/both/deep/file1.txt"))
            .expect("Expected an entry");
        assert_eq!("file1.txt", resolved.name());
        assert_eq!(Some("/both/deep/file1.txt"), resolved.content());
    }

    {
        let resolved =
            resolve_entry(&mut entries, Path::new("/both/deep")).expect("Expected an entry");
        assert_eq!("deep", resolved.name());
        assert!(matches!(resolved, Entry::Dir { .. }));
    }

    {
        let none = resolve_entry(&mut entries, Path::new("/only-remote.txt"));
        assert!(none.is_none());
    }
}

pub struct Dataset {
    pub local: Vec<Entry>,
    pub remote: Vec<Entry>,
    pub mtime_ref: Option<SystemTime>,
}

impl Default for Dataset {
    fn default() -> Self {
        Self {
            local: build::LOCAL.iter().map(|e| (*e).into()).collect(),
            remote: build::REMOTE.iter().map(|e| (*e).into()).collect(),
            mtime_ref: None,
        }
    }
}

impl Dataset {
    pub fn with_mtime_ref(mut self, mtime: SystemTime) -> Self {
        self.mtime_ref = Some(mtime);
        self
    }

    pub fn with_mtime_now(self) -> Self {
        self.with_mtime_ref(SystemTime::now())
    }
}

pub enum Patch {
    Content(PathBuf, String),
    Age(PathBuf, u32),
    Delete(PathBuf),
}

impl Patch {
    pub fn path(&self) -> &Path {
        match self {
            Patch::Content(path, _) => path,
            Patch::Age(path, _) => path,
            Patch::Delete(path) => path,
        }
    }

    pub fn apply(self, root_entries: &mut Vec<Entry>) {
        assert!(self.path().is_absolute());
        match self {
            Patch::Content(path, new_content) => {
                let entry = resolve_entry(root_entries, &path);
                match entry {
                    Some(Entry::File { content, .. }) => {
                        *content = new_content;
                    }
                    _ => panic!("Expected a file entry"),
                }
            }
            Patch::Age(path, new_age) => {
                let entry = resolve_entry(root_entries, &path);
                match entry {
                    Some(Entry::File { age, .. }) => {
                        *age = Some(new_age);
                    }
                    _ => panic!("Expected a file entry"),
                }
            }
            Patch::Delete(path) => {
                let parent_path = path.parent().expect("Expected a parent");
                let file_name = path.file_name().unwrap();

                // parent can be root!
                let parent_entries = if parent_path.is_root() {
                    root_entries
                } else {
                    let parent_entry = resolve_entry(root_entries, &parent_path);
                    if let Some(Entry::Dir { entries, .. }) = parent_entry {
                        entries
                    } else {
                        panic!("Expected a directory as parent entry");
                    }
                };
                let prev_len = parent_entries.len();
                parent_entries.retain(|e| e.name() != file_name);
                assert!(
                    prev_len == parent_entries.len() + 1,
                    "Expected to delete an entry"
                );
            }
        }
    }
}

#[test]
fn test_patch_apply() {
    let mut entries = build::LOCAL.iter().map(|e| (*e).into()).collect::<Vec<_>>();

    Patch::Content("/both/deep/file1.txt".into(), "new test content".into()).apply(&mut entries);
    Patch::Age("/only-local.txt".into(), 42).apply(&mut entries);
    Patch::Delete("/only-local".into()).apply(&mut entries);
    Patch::Delete("/conflict.txt".into()).apply(&mut entries);

    assert_eq!(
        Some("new test content"),
        resolve_entry(&mut entries, Path::new("/both/deep/file1.txt"))
            .unwrap()
            .content()
    );
    assert_eq!(
        Some(42),
        resolve_entry(&mut entries, Path::new("/only-local.txt"))
            .unwrap()
            .age()
    );

    assert!(resolve_entry(&mut entries, Path::new("/only-local")).is_none());
    assert!(resolve_entry(&mut entries, Path::new("/conflict.txt")).is_none());
}

impl Dataset {
    pub fn apply_local(mut self, patch: Patch) -> Self {
        patch.apply(&mut self.local);
        self
    }

    pub fn apply_remote(mut self, patch: Patch) -> Self {
        patch.apply(&mut self.remote);
        self
    }
}

impl Entry {
    pub fn create_fs<'a>(&'a self, path: &'a FsPath, now: Option<SystemTime>) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            match self {
                Entry::Dir { name, entries } => {
                    let path = path.join(name);
                    tokio::fs::create_dir(&path).await.unwrap();
                    for entry in entries.iter() {
                        entry.create_fs(&path, now).await;
                    }
                }
                Entry::File { name, content, age } => {
                    let path = path.join(name);
                    let mut f = tokio::fs::File::create(&path).await.unwrap();
                    f.write(content.as_bytes()).await.unwrap();
                    if let Some(age) = age {
                        let f = f.into_std().await;
                        let now = now.unwrap_or_else(|| SystemTime::now());
                        let age = Duration::from_secs(*age as u64);
                        f.set_modified(now - age).unwrap();
                    }
                }
            }
        })
    }
}

pub trait CreateFs {
    async fn create_fs(&self, root: &FsPath, now: Option<SystemTime>);
}

impl CreateFs for &[Entry] {
    async fn create_fs(&self, root: &FsPath, now: Option<SystemTime>) {
        for entry in self.iter() {
            entry.create_fs(&root, now).await;
        }
    }
}

impl Dataset {
    pub async fn create_fs(
        &self,
        root: &FsPath,
    ) -> (stubs::fs::Stub, CacheStorage<stubs::id::Stub>) {
        use futures::FutureExt;

        let local_root = root.join("local");
        let local = stubs::fs::Stub::new(&local_root, &self.local, self.mtime_ref);

        let remote_root = root.join("remote");
        let remote =
            stubs::id::Stub::new(&remote_root, &self.remote, self.mtime_ref).then(|remote| async {
                CacheStorage::new(remote.unwrap(), CachePersist::Memory).await
            });

        let (local, remote) = tokio::try_join!(local, remote).unwrap();

        (local, remote)
    }
}
