use std::fmt;

use async_stream::try_stream;
use camino::{Utf8Component, Utf8Path, Utf8PathBuf};
use fsync::{self, PathId};
use futures::{Future, Stream};
use tokio::{
    fs::{self, DirEntry},
    io,
};

#[derive(Debug)]
pub struct OutOfTreeSymlink {
    path: Utf8PathBuf,
    target: String,
}

impl fmt::Display for OutOfTreeSymlink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Out of tree symlink: {} -> {}", self.path, self.target)
    }
}

impl std::error::Error for OutOfTreeSymlink {}

fn check_symlink<P1, P2>(link: P1, target: P2) -> Result<(), OutOfTreeSymlink>
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
        return Err(OutOfTreeSymlink {
            path: link.to_owned(),
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
                return Err(OutOfTreeSymlink {
                    path: link.to_owned(),
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

#[derive(Debug, Clone)]
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

impl super::DirEntries for Storage {
    fn dir_entries(
        &self,
        parent_path_id: Option<PathId>,
    ) -> impl Stream<Item = anyhow::Result<fsync::Metadata>> + Send {
        let fs_base = match parent_path_id {
            Some(dir) => self.root.join(dir.path),
            None => self.root.clone(),
        };
        let parent_path = parent_path_id.map(|pid| pid.path);
        try_stream! {
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

impl super::ReadFile for Storage {
    fn read_file<'a>(
        &'a self,
        path_id: PathId<'a>,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead>> + Send + 'a {
        debug_assert!(path_id.path.is_relative());
        let path = self.root.join(path_id.path);
        async move { Ok(tokio::fs::File::open(&path).await?) }
    }
}

impl super::CreateFile for Storage {
    fn create_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<fsync::Metadata>> + Send {
        async move {
            debug_assert!(metadata.path().is_relative());
            let fs_path = self.root.join(metadata.path());
            if fs_path.is_dir() {
                anyhow::bail!("{} exists and is a directory", metadata.path());
            }
            if fs_path.exists() {
                anyhow::bail!("{} already exists", metadata.path());
            }
            {
                tokio::pin!(data);

                let mut f = tokio::fs::File::create(&fs_path).await?;
                tokio::io::copy(&mut data, &mut f).await?;

                if let Some(mtime) = metadata.mtime() {
                    let f = f.into_std().await;
                    f.set_modified(mtime.into())?;
                }
            }
            let fs_metadata = tokio::fs::metadata(&fs_path).await?;
            map_metadata(metadata.path().to_owned(), &fs_metadata, &fs_path).await
        }
    }
}

impl super::Storage for Storage {}

async fn map_direntry(
    parent_path: Option<&Utf8Path>,
    direntry: &DirEntry,
) -> anyhow::Result<fsync::Metadata> {
    let fs_path = Utf8PathBuf::try_from(direntry.path())?;
    let file_name = String::from_utf8(direntry.file_name().into_encoded_bytes())
        .map_err(|err| err.utf8_error())?;
    let path = parent_path
        .map(|p| p.join(&file_name))
        .unwrap_or_else(|| Utf8PathBuf::from(&file_name));
    let metadata = direntry.metadata().await?;
    map_metadata(path, &metadata, &fs_path).await
}

async fn map_metadata(
    path: Utf8PathBuf,
    metadata: &std::fs::Metadata,
    fs_path: &Utf8Path,
) -> anyhow::Result<fsync::Metadata> {
    let metadata = if metadata.is_symlink() {
        let target = tokio::fs::read_link(fs_path).await?;
        let target = Utf8PathBuf::try_from(target)?;
        check_symlink(&path, &target)?;
        fsync::Metadata::Symlink {
            id: path.to_string(),
            path,
            target: target.into_string(),
            size: metadata.len(),
            mtime: metadata.modified().ok().map(|mt| mt.into()),
        }
    } else if metadata.is_file() {
        fsync::Metadata::Regular {
            id: path.to_string(),
            path,
            size: metadata.len(),
            mtime: metadata.modified().map(|mt| mt.into())?,
        }
    } else if metadata.is_dir() {
        fsync::Metadata::Directory {
            id: path.to_string(),
            path,
        }
    } else {
        fsync::Metadata::Special {
            id: path.to_string(),
            path,
        }
    };

    Ok(metadata)
}
