use fsync::path::{FsPath, FsPathBuf};
use futures::future::BoxFuture;
use tokio::{fs, io};

pub fn temp_path(prefix: Option<&str>, ext: Option<&str>) -> FsPathBuf {
    use rand::distributions::Alphanumeric;
    use rand::Rng;

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
