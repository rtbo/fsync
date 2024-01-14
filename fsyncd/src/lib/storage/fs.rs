use std::fmt;

use async_stream::try_stream;
use fsync::path::{self, FsPath, FsPathBuf, Path, PathBuf};
use futures::Stream;
use tokio::{
    fs::{self, DirEntry},
    io,
};

use crate::PersistCache;

#[derive(Debug)]
pub struct OutOfTreeSymlink {
    path: PathBuf,
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
    P1: AsRef<Path>,
    P2: AsRef<Path>,
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
            path::Component::RootDir => {
                unreachable!("unexpected root component in {link:?} -> {target:?}")
            }
            path::Component::CurDir => (),
            path::Component::ParentDir if num_comps <= 0 => {
                return Err(OutOfTreeSymlink {
                    path: link.to_owned(),
                    target: target.to_string(),
                });
            }
            path::Component::ParentDir => num_comps -= 1,
            path::Component::Normal(_) => num_comps += 1,
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
pub struct FileSystem {
    root: FsPathBuf,
}

impl FileSystem {
    /// Build a new filesystem storage.
    /// Panics if [root] is not an absolute path.
    pub fn new<P>(root: P) -> anyhow::Result<Self>
    where
        P: AsRef<FsPath>,
    {
        let root = root.as_ref();
        assert!(root.is_absolute());
        let root = root.canonicalize_utf8()?;
        log::info!("Initializing FS storage in {root}");

        Ok(FileSystem { root })
    }

    pub fn root(&self) -> &FsPath {
        &self.root
    }
}

impl super::DirEntries for FileSystem {
    fn dir_entries(
        &self,
        parent_path: PathBuf,
    ) -> impl Stream<Item = anyhow::Result<fsync::Metadata>> + Send {
        debug_assert!(parent_path.is_absolute());
        let fs_base = self.root.join(parent_path.without_root().as_str());
        log::trace!("listing entries of {fs_base}");
        try_stream! {
            let mut read_dir = fs::read_dir(&fs_base).await?;
            loop {
                match read_dir.next_entry().await? {
                    None => break,
                    Some(direntry) => {
                        yield map_direntry(&parent_path, &direntry).await?;
                    }
                }
            }
        }
    }
}

impl super::ReadFile for FileSystem {
    async fn read_file(&self, path: PathBuf) -> anyhow::Result<impl io::AsyncRead> {
        debug_assert!(path.is_absolute());
        let fs_path = self.root.join(path.without_root().as_str());
        log::trace!("reading {fs_path}");
        Ok(tokio::fs::File::open(&fs_path).await?)
    }
}

impl super::MkDir for FileSystem {
    async fn mkdir(&self, path: &Path, parents: bool) -> anyhow::Result<()> {
        debug_assert!(path.is_absolute());
        let fs_path = self.root.join(path.without_root().as_str());
        log::info!("mkdir {}{}", if parents { "-p " } else { "" }, fs_path);
        if parents {
            tokio::fs::create_dir_all(&fs_path).await?;
        } else {
            tokio::fs::create_dir(&fs_path).await?;
        }
        Ok(())
    }
}

impl super::CreateFile for FileSystem {
    async fn create_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> anyhow::Result<fsync::Metadata> {
        debug_assert!(metadata.path().is_absolute());
        let fs_path = self.root.join(metadata.path().without_root().as_str());
        log::info!("creating {fs_path}");
        if fs_path.is_dir() {
            anyhow::bail!("{} exists and is a direceory: {fs_path}", metadata.path());
        }
        if fs_path.exists() {
            anyhow::bail!("{} already exists here: {fs_path}", metadata.path());
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

impl PersistCache for FileSystem {}

impl super::Storage for FileSystem {}

async fn map_direntry(parent_path: &Path, direntry: &DirEntry) -> anyhow::Result<fsync::Metadata> {
    let fs_path = FsPathBuf::try_from(direntry.path())?;
    let file_name = String::from_utf8(direntry.file_name().into_encoded_bytes())?;
    let path = parent_path.join(&file_name);
    let metadata = direntry.metadata().await?;
    map_metadata(path, &metadata, &fs_path).await
}

async fn map_metadata(
    path: PathBuf,
    metadata: &std::fs::Metadata,
    fs_path: &FsPath,
) -> anyhow::Result<fsync::Metadata> {
    let metadata = if metadata.is_symlink() {
        let target = tokio::fs::read_link(fs_path).await?;
        let target = PathBuf::try_from(target)?;
        check_symlink(&path, &target)?;
        fsync::Metadata::Symlink {
            path,
            target: target.into_string(),
            size: metadata.len(),
            mtime: metadata.modified().ok().map(|mt| mt.into()),
        }
    } else if metadata.is_file() {
        fsync::Metadata::Regular {
            path,
            size: metadata.len(),
            mtime: metadata.modified().map(|mt| mt.into())?,
        }
    } else if metadata.is_dir() {
        fsync::Metadata::Directory { path }
    } else {
        fsync::Metadata::Special { path }
    };

    Ok(metadata)
}
