use std::fs::FileType;
use std::path::{Path, PathBuf, Component};
use std::str;

use tokio::fs::{self, DirEntry};
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::{Stream, StreamExt};

use crate::storage;
use crate::Result;

#[derive(Debug, Clone)]
pub struct Entry {
    path: String,
    name_start: usize,
    file_type: FileType,
    symlink_target: Option<String>,
    mime_type: Option<String>,
}

impl storage::Entry for Entry {
    fn id(&self) -> &str {
        &self.path
    }

    fn name(&self) -> &str {
        &self.path[self.name_start..]
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn entry_type(&self) -> storage::EntryType {
        if self.file_type.is_file() {
            storage::EntryType::Regular
        } else if self.file_type.is_dir() {
            storage::EntryType::Directory
        } else if self.file_type.is_symlink() {
            storage::EntryType::Symlink
        } else {
            storage::EntryType::Special
        }
    }

    fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }

    fn symlink_target(&self) -> Option<&str> {
        self.symlink_target.as_deref()
    }
}

pub struct Storage {
    root: PathBuf,
}

impl Storage {
    /// Build a new filesystem storage.
    /// Panics if [root] is not an absolute path.
    pub fn new<P>(root: P) -> Self
    where
        P: AsRef<Path>,
    {
        let root = root.as_ref();

        assert!(root.is_absolute());

        Storage {
            root: root.canonicalize().unwrap(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    async fn map_entry(&self, entry: &DirEntry, base: Option<&str>) -> Entry {
        let path = match base {
            Some(base) => [base, entry.file_name().to_str().unwrap()].join("/"),
            None => entry.file_name().to_str().unwrap().into(),
        };
        let name_len = entry.file_name().len();
        let name_start = path.len() - name_len;
        let metadata = entry.metadata().await.unwrap();
        let symlink_target = if metadata.is_symlink() {
            let link = entry.path();
            let target = tokio::fs::read_link(&link).await.unwrap();
            Some(self.symlink_target(&link, &target).to_str().unwrap().into())
        } else {
            None
        };

        Entry {
            path,
            name_start,
            file_type: metadata.file_type(),
            symlink_target,
            mime_type: None,
        }
    }

    fn symlink_target<P>(&self, link: P, target: P) -> PathBuf
    where
        P: AsRef<Path>,
    {
        let link = link.as_ref();
        let target = target.as_ref();

        if target.is_absolute() {
            match target.strip_prefix(self.root()) {
                Ok(path) => path.to_owned(),
                Err(_) => panic!("unsupported out of tree symlink"),
            }
        } else {
            let mut from_root: Vec<Component> = vec![];
            for comp in link.parent().unwrap().components().chain(target.components()) {
                match comp {
                    Component::Prefix(pref) => panic!("unexpected prefix component: {pref:?}"),
                    Component::RootDir =>panic!("unexpected root component"),
                    Component::CurDir => (),
                    Component::ParentDir => {
                        assert!(from_root.pop().is_some(), "unsupported out of tree symlink");
                    },
                    Component::Normal(comp) => {
                        from_root.push(Component::Normal(comp))
                    }
                }
            }
            from_root.iter().map(|c| c.as_os_str()).collect()
        }
    }
}

#[test]
fn test_symlink_target() {
    #[cfg(target_os = "windows")]
    let root_path = "C:\\storage";
    #[cfg(not(target_os = "windows"))]
    let root_path = "/storage";

    let storage = Storage{
        root: PathBuf::from(root_path), // bypass canonicalize
    };
    assert_eq!(
        storage.symlink_target("dir/symlink", "actual_file"),
        Path::new("dir/actual_file")
    );
    assert_eq!(
        storage.symlink_target("dir/symlink", "../actual_file"),
        Path::new("actual_file")
    );
    assert_eq!(
        storage.symlink_target("dir/symlink", "../other_dir/actual_file"),
        Path::new("other_dir/actual_file")
    );
}

impl storage::Storage for Storage {
    type E = Entry;

    async fn entries(&self, dir_id: Option<&str>) -> Result<impl Stream<Item = Result<Self::E>>> {
        let base = match dir_id {
            Some(dir) => self.root.join(dir),
            None => self.root.clone(),
        };
        let stream = fs::read_dir(base).await?;
        let stream = ReadDirStream::new(stream).then(move |e| async move {
            match e {
                Ok(entry) => Ok(self.map_entry(&entry, dir_id).await),
                Err(err) => Err(err.into()),
            }
        });
        Ok(stream)
    }
}
