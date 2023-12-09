use std::sync::Arc;

use camino::Utf8PathBuf;
use dashmap::DashMap;
use fsync::{Entry, EntryType, PathId, Storage};
use fsync::{Error, Result};
use futures::future::BoxFuture;
use tokio::task::JoinSet;
use tokio_stream::StreamExt;

pub struct Cache {
    entries: Arc<DashMap<String, CacheEntry>>,
}

pub struct CacheEntry {
    entry: Entry,
    _parent: Option<String>,
    children: Vec<String>,
}

impl Cache {
    pub async fn new_from_storage<S>(storage: Arc<S>) -> Result<Self>
    where
        S: Storage + Send + Sync + 'static,
    {
        let entries = Arc::new(DashMap::new());

        let children = populate_recurse(None, None, entries.clone(), storage).await?;
        entries.insert(
            "".into(),
            CacheEntry {
                entry: Entry::new("".to_string(), Utf8PathBuf::new(), EntryType::Directory),
                _parent: None,
                children,
            },
        );
        Ok(Self { entries })
    }

    pub fn print_tree(&self) {
        self._print_tree(None, 0);
    }

    fn _print_tree(&self, dir_id: Option<&str>, indent: u32) {
        let key = match dir_id {
            Some(dir_id) => dir_id,
            None => "",
        };
        let entry = self.entries.get(key).expect("Could not find dir entry");
        println!("{}{}", "  ".repeat(indent as usize), entry.entry.path());
        for c in entry.children.iter() {
            self._print_tree(Some(c.as_str()), indent + 1);
        }
    }
}

fn populate_recurse<'a, S>(
    dir_id: Option<String>,
    dir_path: Option<String>,
    entries: Arc<DashMap<String, CacheEntry>>,
    storage: Arc<S>,
) -> BoxFuture<'a, Result<Vec<String>>>
where
    S: Storage + Send + Sync + 'static,
{
    Box::pin(async move {
        let dir_id = match (&dir_id, &dir_path) {
            (Some(dir_id), Some(dir_path)) => {
                Some(PathId{id: dir_id, path: dir_path})
            }
            _ => None,
        };
        let dirent = storage.entries(dir_id);
        tokio::pin!(dirent);

        let mut children = Vec::new();
        let mut set = JoinSet::new();

        while let Some(ent) = dirent.next().await {
            let ent = ent?;
            let ent_id = ent.id().to_owned();
            let ent_path = ent.path().to_owned();

            children.push(ent_id.clone());

            let parent = dir_id.map(|pid| pid.id.to_owned());
            let entries = entries.clone();
            let storage = storage.clone();
            set.spawn(async move {
                let children = match ent.typ() {
                    EntryType::Directory => {
                        populate_recurse(Some(ent_id.clone()), Some(ent_path.clone().into()), entries.clone(), storage).await?
                    }
                    _ => Vec::new(),
                };
                entries.insert(
                    ent_id,
                    CacheEntry {
                        entry: ent,
                        _parent: parent,
                        children,
                    },
                );
                Ok::<_, Error>(())
            });
        }

        while let Some(res) = set.join_next().await {
            res.unwrap()?;
        }

        Ok(children)
    })
}
