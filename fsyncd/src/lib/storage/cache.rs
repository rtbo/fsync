use std::{mem, sync::Arc};

use anyhow::Context;
use async_stream::try_stream;
use bincode::Options;
use dashmap::DashMap;
use fsync::{
    path::{Component, FsPath, FsPathBuf, Path, PathBuf},
    Metadata,
};
use futures::{future::BoxFuture, Stream};
use serde::{Deserialize, Serialize};
use tokio::{io, task::JoinSet};
use tokio_stream::StreamExt;

use super::id::{self, IdBuf};
use crate::{PersistCache, SharedProgress};

#[derive(Clone, Debug)]
pub enum CachePersist {
    Memory,
    MemoryAndDisk {
        path: FsPathBuf,
        ignore_initial_cache: bool,
    },
}

impl CachePersist {
    fn try_load_path(&self) -> Option<&FsPath> {
        match self {
            Self::MemoryAndDisk {
                path,
                ignore_initial_cache: false,
            } => Some(path),
            _ => None,
        }
    }

    fn try_save_path(&self) -> Option<&FsPath> {
        match self {
            Self::MemoryAndDisk { path, .. } => Some(path),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStorage<S> {
    entries: Arc<DashMap<PathBuf, CacheNode>>,
    storage: Arc<S>,
    persist: CachePersist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheNode {
    id: Option<IdBuf>,
    metadata: fsync::Metadata,
    children: Vec<String>,
}

impl<S> CacheStorage<S>
where
    S: id::Storage,
{
    pub async fn new(storage: S, persist: CachePersist) -> anyhow::Result<Self> {
        let storage = Arc::new(storage);
        let entries = if let Some(path) = persist.try_load_path() {
            match load_from_disk(path).await {
                Ok(entries) => entries,
                Err(LoadError::Io(_)) => populate_from_storage(storage.clone()).await?,
                Err(LoadError::Bincode(err)) => Err(err)?,
            }
        } else {
            populate_from_storage(storage.clone()).await?
        };
        Ok(Self {
            entries,
            storage,
            persist,
        })
    }
}

impl<S> CacheStorage<S> {
    fn check_path(path: &Path) -> fsync::Result<PathBuf> {
        debug_assert!(path.is_absolute());
        let path = path.normalize()?;
        Ok(path)
    }
}

async fn populate_from_storage<S>(
    storage: Arc<S>,
) -> anyhow::Result<Arc<DashMap<PathBuf, CacheNode>>>
where
    S: id::Storage,
{
    let entries = Arc::new(DashMap::new());
    let children = populate_recurse(None, PathBuf::root(), entries.clone(), storage).await?;
    entries.insert(
        PathBuf::root(),
        CacheNode {
            id: None,
            metadata: fsync::Metadata::root(),
            children,
        },
    );
    Ok(entries)
}

enum LoadError {
    Io(io::Error),
    Bincode(bincode::Error),
}

impl From<io::Error> for LoadError {
    fn from(value: io::Error) -> Self {
        LoadError::Io(value)
    }
}

impl From<bincode::Error> for LoadError {
    fn from(value: bincode::Error) -> Self {
        LoadError::Bincode(value)
    }
}

async fn load_from_disk(path: &FsPath) -> Result<Arc<DashMap<PathBuf, CacheNode>>, LoadError> {
    log::trace!("loading cached entries from {path}");

    let path2 = path.to_owned();

    let handle = tokio::task::spawn_blocking(move || {
        use std::{fs, io::BufReader};

        let reader = fs::File::open(path2)?;
        let reader = BufReader::new(reader);
        let opts = bincode_options();
        let entries: DashMap<PathBuf, CacheNode> = opts.deserialize_from(reader)?;
        Ok::<_, LoadError>(entries)
    });

    let entries = handle.await.unwrap()?;
    log::info!("loaded {} entries from {path}", entries.len());

    Ok(Arc::new(entries))
}

async fn save_to_disc(
    path: &FsPath,
    entries: Arc<DashMap<PathBuf, CacheNode>>,
) -> anyhow::Result<()> {
    use std::{fs, io::BufWriter};

    log::info!("saving {} entries to {path}", entries.len());

    let path = path.to_owned();

    let handle = tokio::task::spawn_blocking(move || {
        let writer = fs::File::create(&path)?;
        let writer = BufWriter::new(writer);
        let opts = bincode_options();
        opts.serialize_into(writer, &*entries)?;
        Ok::<_, anyhow::Error>(())
    });

    handle.await.unwrap()
}

impl<S> super::DirEntries for CacheStorage<S>
where
    S: super::id::DirEntries + Send + Sync + 'static,
{
    fn dir_entries(
        &self,
        parent_path: &Path,
        _progress: Option<&SharedProgress>,
    ) -> impl Stream<Item = fsync::Result<fsync::Metadata>> + Send {
        log::trace!("listing entries for {parent_path}");
        let parent = self.entries.get(parent_path);
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
    async fn read_file(
        &self,
        path: PathBuf,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<impl io::AsyncRead> {
        log::info!("read file {path}");
        let node = self.entries.get(&path);
        if let Some(node) = node {
            if !node.metadata.is_file() {
                fsync::io_bail!("{path} is not a file.");
            }
            let id = node.id.clone();
            let res = self
                .storage
                .read_file(id.expect("File without Id"), progress)
                .await?;
            Ok(res)
        } else {
            fsync::other_bail!("No such entry in the cache: {path}");
        }
    }
}

impl<S> super::MkDir for CacheStorage<S>
where
    S: super::id::MkDir + Send + Sync,
{
    async fn mkdir(
        &self,
        path: &Path,
        parents: bool,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<()> {
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
                    let id = self
                        .storage
                        .mkdir(parent_id.as_deref(), c.as_str(), progress)
                        .await?;
                    let metadata = Metadata::Directory {
                        path: cur.clone(),
                        stat: None,
                    };
                    {
                        let parent = cur.parent().unwrap();
                        let mut parent_entry = self.entries.get_mut(parent).unwrap();
                        parent_entry
                            .children
                            .push(cur.file_name().unwrap().to_string());
                    }
                    self.entries.insert(
                        cur.clone(),
                        CacheNode {
                            id: Some(id.clone()),
                            metadata,
                            children: Vec::new(),
                        },
                    );
                    parent_id = Some(id)
                }
            }
        } else {
            let parent = path.parent().unwrap();
            let id = {
                let mut entry = self
                    .entries
                    .get_mut(parent)
                    .with_context(|| format!("no such entry: {parent}"))?;
                let parent_id = entry.id.clone();
                let id = self
                    .storage
                    .mkdir(parent_id.as_deref(), path.file_name().unwrap(), progress)
                    .await?;
                entry.children.push(path.file_name().unwrap().to_string());
                id
            };
            let metadata = Metadata::Directory {
                path: path.clone(),
                stat: None,
            };
            self.entries.insert(
                path.clone(),
                CacheNode {
                    id: Some(id.clone()),
                    metadata,
                    children: Vec::new(),
                },
            );
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
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<fsync::Metadata> {
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
            .create_file(parent.id.as_deref(), metadata, data, progress)
            .await?;
        mem::drop(parent);

        let node = CacheNode {
            id: Some(id),
            metadata: metadata.clone(),
            children: Vec::new(),
        };
        self.entries.insert(metadata.path().to_owned(), node);
        Ok(metadata)
    }
}

impl<S> super::WriteFile for CacheStorage<S>
where
    S: super::id::WriteFile + Send + Sync,
{
    async fn write_file(
        &self,
        metadata: &fsync::Metadata,
        data: impl io::AsyncRead + Send,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<fsync::Metadata> {
        log::info!("writing file {}", metadata.path());
        debug_assert!(!metadata.path().is_root());
        let path = Self::check_path(metadata.path())?;
        let parent_id = {
            let parent = self
                .entries
                .get(path.parent().expect("non-root path should have parent"))
                .expect("Parent node should be defined");
            parent.id.clone()
        };
        let mut node = self.entries.get_mut(&path).expect("Path should be present");
        let metadata = {
            let id = node
                .id
                .as_deref()
                .expect("Id should be set for non-root path");
            self.storage
                .write_file(id, parent_id.as_deref(), metadata, data, progress)
                .await?
        };
        node.metadata = metadata.clone();
        Ok(metadata)
    }
}

impl<S> super::CopyFile for CacheStorage<S>
where
    S: super::id::Storage,
{
    async fn copy_file(
        &self,
        src: &Path,
        dest: &Path,
        progress: Option<&SharedProgress>,
    ) -> fsync::Result<Metadata> {
        log::info!("copying file {src} to {dest}");
        let src = Self::check_path(src)?;
        let dest = Self::check_path(dest)?;

        debug_assert!(!src.is_root() && !dest.is_root());
        debug_assert!(self.entries.get(&dest).is_none());

        let src_id = {
            let src = self.entries.get(&src).expect("Source should be present");
            src.id
                .as_ref()
                .expect("Id should be set for non-root path")
                .clone()
        };
        let dest_parent_id = {
            let parent = self
                .entries
                .get(dest.parent().expect("non-root path should have parent"))
                .expect("Parent node should be defined");
            parent.id.clone()
        };

        let metadata = {
            let (id, metadata) = self
                .storage
                .copy_file(&src_id, dest_parent_id.as_deref(), &dest, progress)
                .await?;
            let node = CacheNode {
                id: Some(id),
                metadata: metadata.clone(),
                children: Vec::new(),
            };
            self.entries.insert(dest, node);
            metadata
        };

        Ok(metadata)
    }
}

impl<S> super::Delete for CacheStorage<S>
where
    S: super::id::Storage,
{
    async fn delete(&self, path: &Path, progress: Option<&SharedProgress>) -> fsync::Result<()> {
        debug_assert!(!path.is_root());
        log::info!("deleting file {}", path);
        let path = Self::check_path(path)?;
        let id = {
            let node = self.entries.get(&path);
            if node.is_none() {
                fsync::io_bail!("No such entry: {path}");
            }
            let node = node.unwrap();
            if !node.children.is_empty() {
                fsync::io_bail!("{path} is not a file or an empty folder");
            }
            node.id.clone().expect("Non-root entry should have Id")
        };
        self.storage.delete(&id, progress).await?;
        self.entries.remove(&path);
        Ok(())
    }
}

impl<S> crate::PersistCache for CacheStorage<S>
where
    S: super::id::Storage,
{
    async fn persist_cache(&self) -> anyhow::Result<()> {
        if let Some(path) = self.persist.try_save_path() {
            save_to_disc(path, self.entries.clone()).await?;
        }
        Ok(())
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
        let dirent = storage.dir_entries(dir_id.as_deref(), &dir_path, None);
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
