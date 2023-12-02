use fsync::fs;
use fsync::storage::Storage;
use fsync::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let (tx, mut rx) = mpsc::channel(32);
    let st = Arc::new(fs::Storage::new("/home/remi/dev"));
    st.discover(None, Some(2), tx).await?;
    while let Some(entry) = rx.recv().await {
        match &entry {
            Ok(entry) => println!("{}", entry.path()),
            Err(err) => println!("error: {err}"),
        }
    }
    Ok(())
}
