use std::fs::Metadata;
use std::result;

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
    root: Utf8PathBuf,
}

impl Storage {
    /// Build a new filesystem storage.
    /// Panics if [root] is not an absolute path.
    pub fn new<P>(root: P) -> Self
    where
        P: AsRef<Utf8Path>,
    {
        let root = root.as_ref();

        assert!(root.is_absolute());

        Storage {
            root: root.canonicalize_utf8().unwrap(),
        }
    }

    pub fn root(&self) -> &Utf8Path {
        &self.root
    }
}

impl crate::Storage for Storage {
    async fn entry<'a>(&self, path_id: PathId<'a>) -> Result<Entry> {
        let fs_path = self.root.join(path_id.path);
        let path = path_id.path;
        let metadata = std::fs::metadata(&fs_path)?;
        map_metadata(path, &metadata, &fs_path).await
    }

    fn entries(&self, parent_path_id: Option<PathId>) -> impl Stream<Item = Result<Entry>> + Send {
        let fs_base = match parent_path_id {
            Some(dir) => self.root.join(dir.path),
            None => self.root.clone(),
        };
        let parent_path = parent_path_id.map(|pid| pid.path);
        try_stream! {
            if tokio::fs::metadata(&fs_base).await?.is_dir() {
                return;
            }
            let mut read_dir = fs::read_dir(&fs_base).await?;
            loop {
                match read_dir.next_entry().await? {
                    None => break,
                    Some(direntry) => {
                        yield map_direntry(parent_path, &direntry).await?;
                    }
                }
            }
        }
    }
}

async fn map_direntry(parent_path: Option<&Utf8Path>, direntry: &DirEntry) -> Result<Entry> {
    let fs_path = Utf8PathBuf::try_from(direntry.path())?;
    let file_name = String::from_utf8(direntry.file_name().into_encoded_bytes())
        .map_err(|err| err.utf8_error())?;
    let path = parent_path
        .map(|p| p.join(&file_name))
        .unwrap_or_else(|| Utf8PathBuf::from(&file_name));
    let metadata = direntry.metadata().await?;
    map_metadata(&path, &metadata, &fs_path).await
}

async fn map_metadata(path: &Utf8Path, metadata: &Metadata, fs_path: &Utf8Path) -> Result<Entry> {
    let typ = if metadata.is_symlink() {
        let target = tokio::fs::read_link(fs_path).await?;
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

    Ok(Entry::new(path.to_string(), path.to_path_buf(), typ))
}
