use std::time::{Duration, SystemTime};

use fsync::{path::FsPath, stat};
use futures::future::BoxFuture;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
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

impl Entry {
    pub fn create<'a>(&'a self, path: &'a FsPath, now: Option<SystemTime>) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            match self {
                Entry::Dir { name, entries } => {
                    let path = path.join(name);
                    tokio::fs::create_dir(&path).await.unwrap();
                    for entry in entries.iter() {
                        entry.create(&path, now).await;
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

pub trait CreateDataset {
    async fn create_dataset(&self, root: &FsPath, now: Option<SystemTime>);
}

impl CreateDataset for &[Entry] {
    async fn create_dataset(&self, root: &FsPath, now: Option<SystemTime>) {
        for entry in self.iter() {
            entry.create(&root, now).await;
        }
    }
}

#[rustfmt::skip]
pub const LOCAL: &[Entry] = &[
    Entry::File{name: "only-local.txt", content: "/only-local.txt", age: None},
    Entry::File{name: "both.txt", content: "/both.txt - local", age: Some(0)},
    Entry::Dir{name: "only-local", entries: &[
        Entry::File{name: "file1.txt", content: "/only-local/file1.txt", age: None},
        Entry::File{name: "file2.txt", content: "/only-local/file2.txt", age: None},
        Entry::Dir{name: "deep", entries: &[
            Entry::File{name: "file1.txt", content: "/only-local/deep/file1.txt", age: None},
            Entry::File{name: "file2.txt", content: "/only-local/deep/file2.txt", age: None},
        ]},
    ]},
    Entry::Dir{name: "both", entries: &[
        Entry::File{name: "both.txt", content: "/both/both.txt - local", age: Some(20)},
        Entry::File{name: "only-local.txt", content: "/both/only-local.txt", age: None},
        Entry::Dir{name: "deep", entries: &[
            Entry::File{name: "file1.txt", content: "/both/deep/file1.txt", age: Some(0)},
            Entry::File{name: "file2.txt", content: "/both/deep/file2.txt", age: Some(0)},
        ]},
    ]},
];

#[rustfmt::skip]
pub const REMOTE: &[Entry] = &[
    Entry::File{name: "only-remote.txt", content: "/only-remote.txt", age: None},
    Entry::File{name: "both.txt", content: "/both.txt - remote", age: Some(20)},
    Entry::Dir{name: "only-remote", entries: &[
        Entry::File{name: "file1.txt", content: "/only-remote/file1.txt", age: None},
        Entry::File{name: "file2.txt", content: "/only-remote/file2.txt", age: None},
        Entry::Dir{name: "deep", entries: &[
            Entry::File{name: "file1.txt", content: "/only-remote/deep/file1.txt", age: None},
            Entry::File{name: "file2.txt", content: "/only-remote/deep/file2.txt", age: None},
        ]},
    ]},
    Entry::Dir{name: "both", entries: &[
        Entry::File{name: "both.txt", content: "/both/both.txt - remote", age: Some(0)},
        Entry::File{name: "only-remote.txt", content: "/both/only-remote.txt", age: None},
        Entry::Dir{name: "deep", entries: &[
            Entry::File{name: "file1.txt", content: "/both/deep/file1.txt", age: Some(0)},
            Entry::File{name: "file2.txt", content: "/both/deep/file2.txt", age: Some(20)},
        ]},
    ]},
];

pub const NODE_STAT: stat::Node = stat::Node {
    nodes: 23,      // 22 + root
    sync: 7,        // 6 + root
    conflicts: 3,   // all 'both' with different content or age
};
