use futures::future::BoxFuture;
use tokio::sync::mpsc::Sender;
use std::sync::Arc;

use crate::Result;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EntryType {
    Regular,
    Directory,
    Symlink,
    Special,
}

pub trait Entry: Send {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn path(&self) -> &str;
    fn entry_type(&self) -> EntryType;
    fn symlink_target(&self) -> Option<&str>;
    fn mime_type(&self) -> Option<&str>;

    fn is_regular(&self) -> bool {
        self.entry_type() == EntryType::Regular
    }

    fn is_dir(&self) -> bool {
        self.entry_type() == EntryType::Directory
    }

    fn is_symlink(&self) -> bool {
        self.entry_type() == EntryType::Symlink
    }
}

pub trait Storage: Send + Sync + 'static {
    type E: Entry;

    fn entries(
        &self,
        dir_id: Option<&str>,
    ) -> impl std::future::Future<Output = Result<impl Iterator<Item = Result<Self::E>> + Send>> + Send;

    fn discover(
        self: Arc<Self>,
        dir_id: Option<&str>,
        depth: Option<u32>,
        tx: Sender<Result<Self::E>>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {

            if let Some(0) = depth {
                return Ok(())
            }

            let entries = self.entries(dir_id).await?;
            for entry in entries {
                let dir_id = {
                    let mut dir_id: Option<String> = None;
                    if let Ok(entry) = &entry {
                        if entry.is_dir() {
                            dir_id = Some(entry.id().to_owned());
                        }
                    }
                    dir_id
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
