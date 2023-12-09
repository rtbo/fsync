use std::path::{Path, PathBuf};
use std::result;
use std::str;

use async_stream::try_stream;
use camino::{FromPathBufError, Utf8Component, Utf8Path, Utf8PathBuf};
use futures::Stream;
use tokio::fs::{self, DirEntry};

use crate::{Entry, EntryType, PathId, Result};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("name is not Utf8")]
    NotUtf8Name(#[from] FromPathBufError),
    #[error("symlink {path:?} -> {target:?} points out of tree")]
    OutOfTreeSymlink { path: String, target: String },
}

impl From<FromPathBufError> for crate::Error {
    fn from(err: FromPathBufError) -> crate::Error {
        Error::NotUtf8Name(err).into()
    }
}

fn check_symlink<P1, P2>(link: P1, target: P2) -> result::Result<(), Error>
where
    P1: AsRef<Utf8Path>,
    P2: AsRef<Utf8Path>,
{
    let link = link.as_ref();
    let target = target.as_ref();

    debug_assert!(
        !link.is_absolute(),
        "must be called with link relative to storage root"
    );
    if target.is_absolute() {
        return Err(Error::OutOfTreeSymlink {
            path: link.to_string(),
            target: target.to_string(),
        });
    }

    let mut num_comps = 0;

    for comp in link
        .parent()
        .unwrap()
        .components()
        .chain(target.components())
    {
        match comp {
            Utf8Component::Prefix(pref) => panic!("unexpected prefix component: {pref:?}"),
            Utf8Component::RootDir => panic!("unexpected root component in {link:?} -> {target:?}"),
            Utf8Component::CurDir => (),
            Utf8Component::ParentDir if num_comps <= 0 => {
                return Err(Error::OutOfTreeSymlink {
                    path: link.to_string(),
                    target: target.to_string(),
                });
            }
            Utf8Component::ParentDir => num_comps -= 1,
            Utf8Component::Normal(_) => num_comps += 1,
        }
    }

    Ok(())
}

#[test]
fn test_check_symlink() {
    check_symlink("dir/symlink", "actual_file").unwrap();
    check_symlink("dir/symlink", "../actual_file").unwrap();
    check_symlink("dir/symlink", "../other_dir/actual_file").unwrap();
    check_symlink("dir/symlink", "../../actual_file").expect_err("");
    check_symlink("dir/symlink", "/actual_file").expect_err("");
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

    async fn map_entry(&self, entry: &DirEntry, base: Option<&str>) -> Result<Entry> {
        let file_name = Utf8PathBuf::try_from(PathBuf::from(entry.file_name()))?;
        let path = match &base {
            Some(base) => Utf8PathBuf::from([base, file_name.as_str()].join("/")),
            None => file_name,
        };
        let metadata = entry.metadata().await?;
        let typ = if metadata.is_symlink() {
            let target = tokio::fs::read_link(entry.path()).await?;
            let target = Utf8PathBuf::try_from(target)?;
            check_symlink(&path, &target)?;
            EntryType::Symlink {
                target: target.into_string(),
                size: metadata.len(),
                mtime: metadata.modified().ok().map(|mt| mt.into()),
            }
        } else if metadata.is_file() {
            EntryType::Regular {
                size: metadata.len(),
                mtime: metadata.modified().ok().map(|mt| mt.into()),
            }
        } else if metadata.is_dir() {
            EntryType::Directory
        } else {
            EntryType::Special
        };

        Ok(Entry::new(path.clone().into(), path, typ))
    }
}

impl crate::Storage for Storage {
    fn entries(&self, dir_id: Option<PathId>) -> impl Stream<Item = Result<Entry>> + Send {
        let base = match dir_id {
            Some(dir) => self.root.join(dir.path),
            None => self.root.clone(),
        };
        try_stream! {
            let mut read_dir = fs::read_dir(base).await?;
            let base = dir_id.map(|di| di.path);
            loop {
                match read_dir.next_entry().await? {
                    None => break,
                    Some(e) => {
                        let entry = self.map_entry(&e, base).await?;
                        yield entry;
                    }
                }
            }
        }
    }
}
