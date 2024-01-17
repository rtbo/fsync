use anyhow::Context;
use fsync::path::{FsPath, FsPathBuf, Path};
use fsyncd::storage::Storage;
use futures::future::BoxFuture;
use tokio::{fs, io};

pub fn temp_path(prefix: Option<&str>, ext: Option<&str>) -> FsPathBuf {
    use rand::{distributions::Alphanumeric, Rng};

    let mut filename = String::new();
    if let Some(prefix) = prefix {
        filename.push_str(prefix);
        filename.push('-');
    }
    let rnd: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();
    filename.push_str(&rnd);
    if let Some(ext) = ext {
        filename.push('.');
        filename.push_str(ext);
    }
    let mut p = std::env::temp_dir();
    p.push(filename);
    p.try_into().unwrap()
}

pub fn copy_dir_all<'a>(
    src: impl AsRef<FsPath>,
    dst: impl AsRef<FsPath>,
) -> BoxFuture<'a, anyhow::Result<()>> {
    let src = src.as_ref().as_std_path().to_owned();
    let dst = dst.as_ref().as_std_path().to_owned();

    Box::pin(async move {
        fs::create_dir_all(&dst).await?;
        let mut entries = fs::read_dir(&src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let ty = entry.file_type().await?;
            if ty.is_dir() {
                let src: FsPathBuf = entry.path().try_into()?;
                let dst: FsPathBuf = dst.join(entry.file_name()).try_into()?;
                copy_dir_all(&src, &dst).await?;
            } else {
                fs::copy(entry.path(), dst.join(entry.file_name())).await?;
            }
        }
        Ok(())
    })
}

/// Copy the content of file-system `src` to storage `dst`.
/// Both must refer to pre-existing folders.
pub fn copy_dir_all_to_storage<'a, S>(
    storage: &'a S,
    src: &'a FsPath,
    dst: &'a Path,
) -> BoxFuture<'a, anyhow::Result<()>>
where
    S: Storage,
{
    Box::pin(async move {
        let mut entries = fs::read_dir(&src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let fs_metadata = entry.metadata().await?;
            let fs_src: FsPathBuf = entry.path().try_into()?;
            let file_name = entry.file_name();
            let file_name = file_name.to_str().context("UTF-8 filename")?;
            let dst = dst.join(file_name);
            if fs_metadata.is_dir() {
                storage.mkdir(&dst, false).await?;
                copy_dir_all_to_storage(storage, &fs_src, &dst).await?;
            } else {
                let metadata = fsync::Metadata::Regular {
                    path: dst,
                    size: fs_metadata.len(),
                    mtime: fs_metadata.modified()?.into(),
                };
                let data = tokio::fs::File::open(&fs_src).await?;
                storage.create_file(&metadata, data).await?;
            }
        }
        Ok(())
    })
}

pub async fn file_content<R>(read: R) -> anyhow::Result<String>
where
    R: io::AsyncRead,
{
    use io::AsyncReadExt;

    tokio::pin!(read);
    let mut s = String::new();
    read.read_to_string(&mut s).await?;
    Ok(s)
}
