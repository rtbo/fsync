use std::pin::Pin;
use std::sync::Arc;

use futures::future::{Future, FutureExt, BoxFuture};
use tokio::sync::mpsc::Sender;
use tokio_stream::{Stream, StreamExt};

use crate::Result;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EntryType {
    Regular,
    Directory,
    Symlink,
    Special,
}

pub trait Entry {
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

pub trait Storage {
    type E: Entry;

    async fn entries(
        &self,
        dir_id: Option<&str>,
    ) -> Result<impl Iterator<Item = Result<Self::E>>>;

    async fn discover(&self, dir_id: Option<&str>, tx: Sender<Result<Self::E>>) -> Result<()> {
        async fn one_level(dir_id: Option<String>, to_visit: ) {

        }
    }

    // fn discover<'a>(
    //     self: Arc<Self>,
    //     dir_id: Option<&'a str>,
    //     tx: Sender<Result<Self::E>>,
    // ) -> BoxFuture<'a, Result<()>> 
    // where 
    //     Self: 'a,
    //     <Self as Storage>::E: 'static
    // {
    //     Box::pin(async move {
    //         let entries = self.clone().entries(dir_id).await?;

    //         for entry in entries {
    //             let dir_id = {
    //                 let mut dir_id: Option<String> = None;
    //                 if let Ok(entry) = &entry {
    //                     if entry.is_dir() {
    //                         dir_id = Some(entry.id().to_owned());
    //                     }
    //                 }
    //                 dir_id
    //             };

    //             tx.send(entry).await.unwrap();

    //             if let Some(dir_id) = dir_id {
    //                 let tx = tx.clone();
    //                 let this = self.clone();
    //                 tokio::spawn(async move { 
    //                     this.discover(Some(&dir_id), tx).await;
    //                 });
    //             }
    //         }
    //         Ok(())
    //     })
    // }


    // fn discover<'a>(
    //     self: Pin<&'a Self>,
    //     dir_id: Option<&'a str>,
    //     tx: Sender<Result<Self::E>>,
    // ) -> BoxFuture<'a, Result<()>>
    // where
    //     <Self as Storage>::E: 'a,
    //     Self: 'a,
    // {
    //     let dir_id = dir_id.map(|s| s.to_owned());
    //     async move {
    //         let entries = self.entries(dir_id.as_deref()).await?;
    //         let mut entries = Box::pin(entries);
    //         while let Some(entry) = entries.next().await {
    //             if let Ok(entry) = &entry {
    //                 if entry.is_dir() {
    //                     let dir_id = entry.id().to_owned();
    //                     let tx2 = tx.clone();
    //                     tokio::spawn(async move {
    //                         self.discover(Some(&dir_id), tx2).await.unwrap();
    //                     });
    //                 }
    //             }
    //             tx.send(entry).await.unwrap();
    //         }
    //         Ok(())
    //     }
    //     .boxed()
    // }
}

// fn discover_wrapper<S>(storage: &'static S, dir_id: Option<String>, tx: Sender<Result<<S as Storage>::E>>) -> BoxFuture<'static, ()>
// where
//     S: Storage
// {
//     Box::pin(async move {
//         let entries = storage.entries(dir_id.as_deref()).await.unwrap();
//         let mut entries = Box::pin(entries);
//         // tokio::pin!(entries);
//         while let Some(entry) = entries.next().await {
//             if let Ok(entry) = &entry {
//                 if entry.is_dir() {
//                     let dir_id = entry.id().to_owned();
//                     let tx2 = tx.clone();
//                     // tokio::spawn(async move {

//                     // });
//                 }
//             }
//         }
//     })
// }
