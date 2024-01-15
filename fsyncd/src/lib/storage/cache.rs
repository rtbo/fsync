use std::sync::Arc;

use anyhow::Context;
use async_stream::try_stream;
use bincode::Options;
use dashmap::DashMap;
use fsync::path::{Component, FsPathBuf};
use fsync::path::{Path, PathBuf};
use futures::{future::BoxFuture, Stream};
use serde::{Deserialize, Serialize};
use tokio::{io, task::JoinSet};
use tokio_stream::StreamExt;

use super::id::IdBuf;
use crate::PersistCache;

#[derive(Debug, Clone)]
pub struct CacheStorage<S> {
    entries: Arc<DashMap<PathBuf, CacheNode>>,
    storage: Arc<S>,
    path: FsPathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheNode {
    id: Option<IdBuf>,
    metadata: fsync::Metadata,
    children: Vec<String>,
}

impl<S> CacheStorage<S>
where
    S: super::id::Storage,
{
    pub fn new(storage: S, path: FsPathBuf) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            storage: Arc::new(storage),
            path,
        }
    }

    pub async fn load_from_disk(&mut self) -> anyhow::Result<()> {
        use std::fs;
        use std::io::BufReader;

        let path = self.path.clone();
        log::info!("loading cached entries from {path}");

        let handle = tokio::task::spawn_blocking(move || {
            let reader = fs::File::open(path)?;
            let reader = BufReader::new(reader);
            let opts = bincode_options();
            let entries: DashMap<PathBuf, CacheNode> = opts.deserialize_from(reader)?;
            Ok::<_, anyhow::Error>(entries)
        });

        let entries = handle.await.unwrap()?;
        log::trace!("loaded {} entries", entries.len());
        self.entries = Arc::new(entries);
        Ok(())
    }

    pub async fn save_to_disc(&self) -> anyhow::Result<()> {
        use std::fs;
        use std::io::BufWriter;

        let path = self.path.clone();
        log::info!("saving cached entries to {path}");

        let entries = self.entries.clone();
        log::trace!("saving {} entries", entries.len());

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
    S: super::id::DirEntries + Send + Sync + 'static,
{
    pub async fn populate_from_entries(&mut self) -> anyhow::Result<()> {
        let entries = Arc::new(DashMap::new());
        let children =
            populate_recurse(None, PathBuf::root(), entries.clone(), self.storage.clone()).await?;
        entries.insert(
            PathBuf::root(),
            CacheNode {
                id: None,
                metadata: fsync::Metadata::root(),
                children,
            },
        );
        self.entries = entries;
        Ok(())
    }
}

impl<S> super::DirEntries for CacheStorage<S>
where
    S: super::id::DirEntries + Send + Sync + 'static,
{
    fn dir_entries(
        &self,
        parent_path: PathBuf,
    ) -> impl Stream<Item = anyhow::Result<fsync::Metadata>> + Send {
        log::trace!("listing entries for {parent_path}");
        let parent = self.entries.get(&parent_path);
        try_stream! {
            if let Some(parent) = parent {
                for c in parent.children.iter() {
                    let c_key = parent.metadata.path().join(c);
                    let c_ent = self.entries.get(&c_key).unwrap();
                    yield c_ent.metadata.clone();
                }
            }
        }
    }
}

impl<S> super::ReadFile for CacheStorage<S>
where
    S: super::id::ReadFile + Sync + Send,
{
    async fn read_file(&self, path: PathBuf) -> anyhow::Result<impl io::AsyncRead> {
        log::info!("read file {path}");
        let node = self.entries.get(&path);
        if let Some(node) = node {
            if !node.metadata.is_file() {
                anyhow::bail!("{path} is not a file.");
            }
            let id = node.id.clone();
            let res = self.storage.read_file(id.expect("File without Id")).await?;
            Ok(res)
        } else {
            anyhow::bail!("No such entry in the cache: {path}");
        }
    }
}

impl<S> super::MkDir for CacheStorage<S>
where
    S: super::id::MkDir + Send + Sync,
{
    async fn mkdir(&self, path: &Path, parents: bool) -> anyhow::Result<()> {
        debug_assert!(path.is_absolute());
        let path = path.normalize()?;
        if path.is_root() {
            return Ok(());
        }
        if parents {
            let mut parent_id = None;
            let mut cur = PathBuf::new();
            for c in path.components() {
                match c {
                    Component::CurDir | Component::ParentDir => unreachable!(),
                    Component::RootDir | Component::Normal(_) => cur.push(c.as_str()),
                }
                if let Some(entry) = self.entries.get(&cur) {
                    parent_id = entry.id.clone();
                } else {
                    let id = self.storage.mkdir(parent_id.as_deref(), c.as_str()).await?;
                    parent_id = Some(id)
                }
            }
        } else {
            let parent = path.parent().unwrap();
            let entry = self
                .entries
                .get(parent)
                .with_context(|| format!("no such entry: {parent}"))?;
            let parent_id = entry.id.clone();
            self.storage
                .mkdir(parent_id.as_deref(), path.file_name().unwrap())
                .await?;
        }
        Ok(())
    }
}

impl<S> super::CreateFile for CacheStorage<S>
where
    S: super::id::CreateFile + Send + Sync,
{
    async fn create_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
    ) -> anyhow::Result<fsync::Metadata> {
        log::info!("creating file {}", metadata.path());

        debug_assert!(metadata.path().is_absolute() && !metadata.path().is_root());
        let parent = metadata.path().parent().unwrap();
        let parent = self.entries.get(parent).with_context(|| {
            format!(
                "Attempt to create file {} in non-existing parent!",
                metadata.path()
            )
        })?;
        let (id, metadata) = self
            .storage
            .create_file(parent.id.as_deref(), metadata, data)
            .await?;
        let node = CacheNode {
            id: Some(id),
            metadata: metadata.clone(),
            children: Vec::new(),
        };
        self.entries.insert(metadata.path().to_owned(), node);
        Ok(metadata)
    }
}

impl<S> crate::PersistCache for CacheStorage<S>
where
    S: super::id::Storage,
{
    async fn persist_cache(&self) -> anyhow::Result<()> {
        self.save_to_disc().await
    }
}

impl<S> crate::Shutdown for CacheStorage<S>
where
    S: super::id::Storage,
{
    async fn shutdown(&self) -> anyhow::Result<()> {
        let fut1 = self.persist_cache();
        let fut2 = self.storage.shutdown();
        tokio::try_join!(fut1, fut2)?;
        Ok(())
    }
}

impl<S> super::Storage for CacheStorage<S> where S: super::id::Storage {}

fn bincode_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

fn populate_recurse<'a, S>(
    dir_id: Option<IdBuf>,
    dir_path: PathBuf,
    entries: Arc<DashMap<PathBuf, CacheNode>>,
    storage: Arc<S>,
) -> BoxFuture<'a, anyhow::Result<Vec<String>>>
where
    S: super::id::DirEntries + Send + Sync + 'static,
{
    Box::pin(async move {
        let dirent = storage.dir_entries(dir_id.clone(), dir_path.clone());
        tokio::pin!(dirent);

        let mut children: Vec<String> = Vec::new();
        let mut set = JoinSet::new();

        while let Some(ent) = dirent.next().await {
            let (id, metadata) = ent?;

            children.push(
                metadata
                    .path()
                    .file_name()
                    .expect("no file name?")
                    .to_owned(),
            );

            let path = metadata.path().to_owned();

            let entries = entries.clone();
            let storage = storage.clone();
            set.spawn(async move {
                let children = match &metadata {
                    fsync::Metadata::Directory { .. } => {
                        populate_recurse(Some(id.clone()), path.clone(), entries.clone(), storage)
                            .await?
                    }
                    _ => Vec::new(),
                };
                entries.insert(
                    path,
                    CacheNode {
                        id: Some(id),
                        metadata,
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
