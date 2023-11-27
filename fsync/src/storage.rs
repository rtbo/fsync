use futures::future::BoxFuture;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryType {
    Regular { mime_type: String },
    Directory,
    Symlink { target: String, mime_type: String },
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub path: String,
    pub typ: EntryType,
}

pub trait Storage: Send + Sync + 'static {
    fn entries(
        &self,
        dir_id: Option<&str>,
    ) -> impl std::future::Future<Output = Result<impl Iterator<Item = Result<Entry>> + Send>> + Send;

    fn discover(
        self: Arc<Self>,
        dir_id: Option<&str>,
        depth: Option<u32>,
        tx: Sender<Result<Entry>>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if let Some(0) = depth {
                return Ok(());
            }

            let entries = self.entries(dir_id).await?;
            for entry in entries {
                let dir_id = match &entry {
                    Ok(Entry {
                        id,
                        typ: EntryType::Directory,
                        ..
                    }) => Some(id.clone()),
                    _ => None,
                };

                tx.send(entry).await.unwrap();

                if let Some(dir_id) = dir_id {
                    let tx = tx.clone();
                    let this = self.clone();
                    tokio::spawn(async move {
                        this.discover(Some(&dir_id), depth.map(|depth| depth - 1), tx)
                            .await
                            .unwrap();
                    });
                }
            }
            Ok(())
        })
    }
}
