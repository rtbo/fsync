use tokio_stream::StreamExt;

use fsync::Result;
use fsync::fs;
use fsync::storage::Storage;

#[tokio::main]
async fn main() -> Result<()> {
    let st = fs::Storage::new("/home/remi");
    let entries = st.entries(Some("dev")).await?;
    tokio::pin!(entries); 
    while let Some(entry) = entries.next().await {
        println!("{:?}", entry?);
    }
    Ok(())
}
