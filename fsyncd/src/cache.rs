use std::ops::Deref;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use dashmap::DashMap;
use fsync::{Entry, EntryType, PathId, Storage};
use fsync::{Error, Result};
use futures::future::BoxFuture;
use tokio::task::JoinSet;
use tokio_stream::StreamExt;

use crate::config::PatternList;

pub struct Cache {
    entries: Arc<DashMap<Utf8PathBuf, CacheEntry>>,
}

pub struct CacheEntry {
    pub entry: Entry,
    pub parent: Option<Utf8PathBuf>,
    pub children: Vec<String>,
}

impl Cache {
    pub async fn new_from_storage<S>(storage: Arc<S>, ignored: Arc<PatternList>) -> Result<Self>
    where
        S: Storage + Send + Sync + 'static,
    {
        let entries = Arc::new(DashMap::new());

        let children = populate_recurse(None, None, entries.clone(), storage, ignored).await?;
        entries.insert(
            Utf8PathBuf::new(),
            CacheEntry {
                entry: Entry::new("".to_string(), Utf8PathBuf::new(), EntryType::Directory),
                parent: None,
                children,
            },
        );
        Ok(Self { entries })
    }

    pub fn entry<'a>(
        &'a self,
        path: Option<&Utf8Path>,
    ) -> Option<impl Deref<Target = CacheEntry> + 'a> {
        let key = path.unwrap_or_else(|| Utf8Path::new(""));
        self.entries.get(key)
    }

    // pub fn print_tree(&self) {
    //     self._print_tree(None, 0);
    // }

    // fn _print_tree(&self, dir_id: Option<&str>, indent: u32) {
    //     let key = match dir_id {
    //         Some(dir_id) => dir_id,
    //         None => "",
    //     };
    //     let entry = self.entries.get(key).expect("Could not find dir entry");
    //     println!("{}{}", "  ".repeat(indent as usize), entry.entry.path());
    //     for c in entry.children.iter() {
    //         self._print_tree(Some(c.as_str()), indent + 1);
    //     }
    // }
}

fn populate_recurse<'a, 'b, S>(
    dir_id: Option<String>,
    dir_path: Option<Utf8PathBuf>,
    entries: Arc<DashMap<Utf8PathBuf, CacheEntry>>,
    storage: Arc<S>,
    ignored: Arc<PatternList>,
) -> BoxFuture<'a, Result<Vec<String>>>
where
    S: Storage + Send + Sync + 'static,
{
    Box::pin(async move {
        let dir_id = match (&dir_id, &dir_path) {
            (Some(dir_id), Some(dir_path)) => Some(PathId {
                id: dir_id,
                path: dir_path,
            }),
            _ => None,
        };
        let dirent = storage.entries(dir_id);
        tokio::pin!(dirent);

        let mut children = Vec::new();
        let mut set = JoinSet::new();

        while let Some(ent) = dirent.next().await {
            let ent = ent?;
            if ignored.matches_with(ent.path()) {
                continue;
            }

            let ent_id = ent.id().to_owned();
            let ent_path = ent.path().to_owned();

            children.push(ent.name().to_owned());

            let parent = dir_id.map(|pid| pid.path.to_owned());
            let entries = entries.clone();
            let storage = storage.clone();
            let ignored = ignored.clone();
            set.spawn(async move {
                let children = match ent.typ() {
                    EntryType::Directory => {
                        populate_recurse(
                            Some(ent_id.clone()),
                            Some(ent_path.clone()),
                            entries.clone(),
                            storage,
                            ignored,
                        )
                        .await?
                    }
                    _ => Vec::new(),
                };
                entries.insert(
                    ent_path,
                    CacheEntry {
                        entry: ent,
                        parent,
                        children,
                    },
                );
                Ok::<_, Error>(())
            });
        }

        while let Some(res) = set.join_next().await {
            res.unwrap()?;
        }

        children.sort_unstable();
        Ok(children)
    })
}
