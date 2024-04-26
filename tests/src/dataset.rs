use std::time::{Duration, SystemTime};

use fsync::{path::FsPath, stat};
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
