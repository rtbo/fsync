use std::sync::Arc;

use async_stream::try_stream;
use bincode::Options;
use camino::{Utf8Path, Utf8PathBuf};
use dashmap::DashMap;
use futures::{future::BoxFuture, Future, Stream};
use serde::{Deserialize, Serialize};
use tokio::{io, task::JoinSet};
use tokio_stream::StreamExt;

use fsync::{Entry, EntryType, PathId, PathIdBuf};

#[derive(Debug, Clone)]
pub struct CacheStorage<S> {
    entries: Arc<DashMap<String, CacheNode>>,
    storage: Arc<S>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheNode {
    entry: Entry,
    _parent: Option<String>,
    children: Vec<String>,
}

impl<S> CacheStorage<S>
where
    S: super::Storage,
{
    pub fn new(storage: S) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            storage: Arc::new(storage),
        }
    }

    pub async fn load_from_disk(&mut self, path: &Utf8Path) -> anyhow::Result<()> {
        use std::fs;
        use std::io::BufReader;

        let path = path.to_owned();

        let handle = tokio::task::spawn_blocking(move || {
            let reader = fs::File::open(path)?;
            let reader = BufReader::new(reader);
            let opts = bincode_options();
            let entries: DashMap<String, CacheNode> = opts.deserialize_from(reader)?;
            Ok::<_, anyhow::Error>(entries)
        });

        let entries = handle.await.unwrap()?;
        self.entries = Arc::new(entries);
        Ok(())
    }

    pub async fn save_to_disc(&self, path: &Utf8Path) -> anyhow::Result<()> {
        use std::fs;
        use std::io::BufWriter;

        let path = path.to_owned();
        let entries = self.entries.clone();

        let handle = tokio::task::spawn_blocking(move || {
            let writer = fs::File::create(&path)?;
            let writer = BufWriter::new(writer);
            let opts = bincode_options();
            opts.serialize_into(writer, &*entries)?;
            Ok::<_, anyhow::Error>(())
        });

        handle.await.unwrap()
    }
}

impl<S> CacheStorage<S>
where
    S: super::DirEntries + Send + Sync + 'static,
{
    pub async fn populate_from_entries(&mut self) -> anyhow::Result<()> {
        let entries = Arc::new(DashMap::new());
        let children = populate_recurse(None, entries.clone(), self.storage.clone()).await?;
        entries.insert(
            String::new(),
            CacheNode {
                entry: Entry::new("".to_string(), Utf8PathBuf::new(), EntryType::Directory),
                _parent: None,
                children,
            },
        );
        self.entries = entries;
        Ok(())
    }
}

impl<S> super::DirEntries for CacheStorage<S>
where
    S: super::DirEntries + Send + Sync + 'static,
{
    fn dir_entries(
        &self,
        parent_path_id: Option<PathId>,
    ) -> impl Stream<Item = anyhow::Result<Entry>> + Send {
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

impl<S> super::ReadFile for CacheStorage<S>
where
    S: super::ReadFile + Sync + Send,
{
    fn read_file<'a>(
        &'a self,
        path_id: PathId<'a>,
    ) -> impl Future<Output = anyhow::Result<impl io::AsyncRead>> + Send + 'a {
        async move { self.storage.read_file(path_id).await }
    }
}

impl<S> super::CreateFile for CacheStorage<S>
where
    S: super::CreateFile + Send + Sync,
{
    fn create_file(
        &self,
        metadata: &Entry,
        data: impl io::AsyncRead + Send,
    ) -> impl Future<Output = anyhow::Result<Entry>> + Send {
        async move { self.storage.create_file(metadata, data).await }
    }
}

impl<S> super::Storage for CacheStorage<S> where S: super::Storage {}

fn bincode_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

fn populate_recurse<'a, S>(
    dir_path_id: Option<PathIdBuf>,
    entries: Arc<DashMap<String, CacheNode>>,
    storage: Arc<S>,
) -> BoxFuture<'a, anyhow::Result<Vec<String>>>
where
    S: super::DirEntries + Send + Sync + 'static,
{
    Box::pin(async move {
        let dirent = storage.dir_entries(dir_path_id.as_ref().map(|dpi| dpi.as_path_id()));
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
                Ok::<_, anyhow::Error>(())
            });
        }

        while let Some(res) = set.join_next().await {
            res.unwrap()?;
        }

        children.sort_unstable();
        Ok(children)
    })
}
