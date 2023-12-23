use std::sync::Arc;

use async_stream::try_stream;
use camino::Utf8PathBuf;
use dashmap::DashMap;
use futures::{future::BoxFuture, Stream};
use tokio::task::JoinSet;
use tokio_stream::StreamExt;

use crate::{Entry, EntryType, PathId, PathIdBuf, Storage};

#[derive(Debug, Clone)]
pub struct CacheStorage<S> {
    entries: Arc<DashMap<String, CacheNode>>,
    storage: Arc<S>,
}

#[derive(Debug, Clone)]
struct CacheNode {
    entry: Entry,
    _parent: Option<String>,
    children: Vec<String>,
}

impl<S> CacheStorage<S>
where
    S: Storage + Send + Sync + 'static,
{
    pub fn new(storage: S) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            storage: Arc::new(storage),
        }
    }

    pub async fn populate(&self) -> crate::Result<()> {
        let children = populate_recurse(None, self.entries.clone(), self.storage.clone()).await?;
        self.entries.insert(
            String::new(),
            CacheNode {
                entry: Entry::new("".to_string(), Utf8PathBuf::new(), EntryType::Directory),
                _parent: None,
                children,
            },
        );
        Ok(())
    }
}

impl<S> crate::Storage for CacheStorage<S>
where
    S: Storage + Sync + Send + 'static,
{
    async fn entry<'a>(&self, path_id: PathId<'a>) -> crate::Result<Entry> {
        let ent = self.entries.get(path_id.id).unwrap();
        Ok(ent.entry.clone())
    }

    fn entries(
        &self,
        parent_path_id: Option<PathId>,
    ) -> impl Stream<Item = crate::Result<Entry>> + Send {
        let parent_key = parent_path_id.map(|pid| pid.id).unwrap_or("");
        let parent = self.entries.get(parent_key).unwrap();
        try_stream! {
            for c in parent.children.iter() {
                let c_ent = self.entries.get(c).unwrap();
                yield c_ent.entry.clone();
            }
        }
    }
}

fn populate_recurse<'a, S>(
    dir_path_id: Option<PathIdBuf>,
    entries: Arc<DashMap<String, CacheNode>>,
    storage: Arc<S>,
) -> BoxFuture<'a, crate::Result<Vec<String>>>
where
    S: Storage + Send + Sync + 'static,
{
    Box::pin(async move {
        let dirent = storage.entries(dir_path_id.as_ref().map(|dpi| dpi.as_path_id()));
        tokio::pin!(dirent);

        let mut children = Vec::new();
        let mut set = JoinSet::new();

        while let Some(ent) = dirent.next().await {
            let ent = ent?;

            children.push(ent.id().to_owned());

            let ent_path_id = ent.path_id_buf();
            let parent_id = dir_path_id.as_ref().map(|dpi| dpi.id.clone());
            let entries = entries.clone();
            let storage = storage.clone();
            set.spawn(async move {
                let children = match ent.typ() {
                    EntryType::Directory => {
                        populate_recurse(Some(ent_path_id), entries.clone(), storage).await?
                    }
                    _ => Vec::new(),
                };
                entries.insert(
                    ent.id().to_owned(),
                    CacheNode {
                        entry: ent,
                        _parent: parent_id,
                        children,
                    },
                );
                Ok::<_, crate::Error>(())
            });
        }

        while let Some(res) = set.join_next().await {
            res.unwrap()?;
        }

        children.sort_unstable();
        Ok(children)
    })
}
