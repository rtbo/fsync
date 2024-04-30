use std::{
    collections::HashSet,
    time::{Duration, SystemTime},
    vec,
};

use fsync::path::{FsPath, Path, PathBuf};
use fsyncd::storage::cache::{CachePersist, CacheStorage};
use tokio::io::AsyncWriteExt;

use crate::stubs;

#[derive(Debug, Clone)]
pub enum Entry {
    /// A directory
    /// Generally for empty dir as `File` will generate parent dirs
    Dir(PathBuf),
    /// A regular file
    File {
        /// Absolute path to the file
        /// (root refers to storage root, not FS root)
        path: PathBuf,
        /// Content of the file
        content: Vec<u8>,
        /// Age of the file in seconds
        /// Age is set in the past, relative to the start time of the test execution.
        age: Option<u32>,
    },
}

#[allow(dead_code)]
impl Entry {
    pub fn dir<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self::Dir(path.as_ref().into())
    }

    pub fn empty_file<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self::File {
            path: path.as_ref().into(),
            content: vec![],
            age: None,
        }
    }

    pub fn txt_file<P, C>(path: P, content: C) -> Self
    where
        P: AsRef<Path>,
        C: AsRef<str>,
    {
        Self::File {
            path: path.as_ref().into(),
            content: content.as_ref().into(),
            age: None,
        }
    }

    pub fn bin_file<P, C>(path: P, content: C) -> Self
    where
        P: AsRef<Path>,
        C: AsRef<[u8]>,
    {
        Self::File {
            path: path.as_ref().into(),
            content: content.as_ref().into(),
            age: None,
        }
    }

    pub fn file_with_path_content<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        let content = path.as_ref().as_str().into();
        Self::File {
            path: path.as_ref().into(),
            content,
            age: None,
        }
    }

    pub fn with_age(self, age: u32) -> Self {
        match self {
            Self::File { path, content, .. } => Self::File {
                path,
                content,
                age: Some(age),
            },
            _ => panic!("Expected a file entry!"),
        }
    }
}

impl Entry {
    fn path(&self) -> &Path {
        match self {
            Entry::Dir(path) => path,
            Entry::File { path, .. } => path,
        }
    }

    fn has_age(&self) -> bool {
        matches!(self, Entry::File { age: Some(_), .. })
    }
}

pub struct Dataset {
    pub local: Vec<Entry>,
    pub remote: Vec<Entry>,
}

impl Dataset {
    pub fn empty() -> Self {
        Self {
            local: vec![],
            remote: vec![],
        }
    }

    fn assert_no_collision(&self) {
        fn no_collision(entries: &[Entry]) {
            let mut paths = HashSet::new();
            paths.insert(PathBuf::root());
            for e in entries {
                assert!(paths.insert(e.path().to_owned()), "path collision: {e:?}");
            }
        }
        no_collision(&self.local);
        no_collision(&self.remote);
    }

    fn assert_all_absolute(&self) {
        fn all_absolute(entries: &[Entry]) -> bool {
            entries.iter().all(|e| e.path().is_absolute())
        }
        assert!(
            all_absolute(&self.local),
            "all local paths are not absolute"
        );
        assert!(
            all_absolute(&self.remote),
            "all remote paths are not absolute"
        );
    }

    fn has_age(&self) -> bool {
        fn has(entries: &[Entry]) -> bool {
            entries.iter().any(Entry::has_age)
        }
        has(&self.local) || has(&self.remote)
    }
}

impl Entry {
    pub async fn create_fs<'a>(&'a self, root: &'a FsPath, now: Option<SystemTime>) {
        match self {
            Entry::Dir(path) => {
                let path = root.join(path.without_root().as_str());
                tokio::fs::create_dir_all(&path).await.unwrap();
            }
            Entry::File { path, content, age } => {
                let path = root.join(path.without_root().as_str());
                tokio::fs::create_dir_all(path.parent().unwrap())
                    .await
                    .unwrap();

                let mut f = tokio::fs::File::create(&path).await.unwrap();
                f.write(content).await.unwrap();

                // `now` can be set while `age` is not set. 
                // if `age` is set however, `now` must be set.
                assert!(age.is_none() || now.is_some());

                if let Some(now) = now {
                    let f = f.into_std().await;
                    let age = age.unwrap_or(0);
                    let age = Duration::from_secs(age as u64);
                    f.set_modified(now - age).unwrap();
                }
            }
        }
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

        self.assert_no_collision();
        self.assert_all_absolute();

        let now = if self.has_age() {
            Some(SystemTime::now())
        } else {
            None
        };

        let local_root = root.join("local");
        let local = stubs::fs::Stub::new(&local_root, &self.local, now);

        let remote_root = root.join("remote");
        let remote = stubs::id::Stub::new(&remote_root, &self.remote, now).then(|remote| async {
            CacheStorage::new(remote.unwrap(), CachePersist::Memory).await
        });

        let (local, remote) = tokio::try_join!(local, remote).unwrap();

        (local, remote)
    }
}
