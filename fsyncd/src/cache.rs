use std::sync::Arc;

use camino::Utf8PathBuf;
use dashmap::DashMap;
use fsync::storage::{Entry, EntryType, Storage};
use fsync::{Error, Result};
use futures::future::BoxFuture;
use tokio::task::JoinSet;

pub struct Cache {
    entries: Arc<DashMap<String, CacheEntry>>,
}

pub struct CacheEntry {
    entry: Entry,
    _parent: Option<String>,
    children: Vec<String>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
        }
    }

    pub async fn populate<S>(&self, storage: Arc<S>) -> Result<()>
    where
        S: Storage,
    {
        let children = populate_recurse(None, self.entries.clone(), storage).await?;
        self.entries.insert(
            "".into(),
            CacheEntry {
                entry: Entry::new("".to_string(), Utf8PathBuf::new(), EntryType::Directory),
                _parent: None,
                children,
            },
        );
        Ok(())
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
    entries: Arc<DashMap<String, CacheEntry>>,
    storage: Arc<S>,
) -> BoxFuture<'a, Result<Vec<String>>>
where
    S: Storage,
{
    Box::pin(async move {
        let dirent = storage.entries(dir_id.as_deref()).await?;

        let mut children = Vec::new();
        let mut set = JoinSet::new();

        for ent in dirent {
            let ent = ent?;
            let ent_id = ent.id().to_owned();
            children.push(ent_id.clone());

            let parent = dir_id.clone();
            let entries = entries.clone();
            let storage = storage.clone();
            set.spawn(async move {
                let children = match ent.typ() {
                    EntryType::Directory => {
                        populate_recurse(Some(ent_id.clone()), entries.clone(), storage).await?
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
