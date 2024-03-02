use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv6Addr},
    ops::Bound,
    sync::Arc,
};

use fsync::{
    self,
    loc::inst,
    path::{Path, PathBuf},
    Error, Fsync, Location, Metadata, OpRes, Operation, PathError, StorageDir,
};
use futures::{
    future,
    prelude::*,
    stream::{AbortHandle, AbortRegistration, Abortable},
};
use tarpc::{
    context::Context,
    server::{self, incoming::Incoming, Channel},
    tokio_serde::formats::Bincode,
};
use tokio::sync::RwLock;

use crate::{
    storage,
    tree::{self, DiffTree},
};

#[derive(Debug)]
pub struct Service<L, R> {
    local: L,
    remote: R,
    tree: DiffTree,
    conflicts: RwLock<BTreeSet<PathBuf>>,
    abort_handle: RwLock<Option<AbortHandle>>,
}

impl<L, R> Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    pub async fn new(local: L, remote: R) -> anyhow::Result<Self> {
        let tree = DiffTree::build(&local, &remote).await?;

        let mut conflicts = BTreeSet::new();

        for node in tree.entries() {
            if let tree::Entry::Sync {
                conflict: Some(_), ..
            } = node.entry()
            {
                let path = node.key().to_path_buf();
                conflicts.insert(path);
            }
        }

        Ok(Self {
            local,
            remote,
            tree,
            conflicts: RwLock::new(conflicts),
            abort_handle: RwLock::new(None),
        })
    }
}

impl<L, R> Service<L, R> {
    fn check_path(path: &Path) -> Result<PathBuf, PathError> {
        if path.is_relative() {
            Err(PathError::Illegal(
                path.to_owned(),
                Some("Expected an absolute path".to_string()),
            ))
        } else {
            Ok(path.normalize()?)
        }
    }

    fn check_node(&self, path: &Path) -> fsync::Result<tree::EntryNode> {
        let path = Self::check_path(path)?;
        let node = self.tree.entry(&path);
        let node = node.ok_or_else(|| fsync::PathError::NotFound(path, None))?;
        Ok(node)
    }

    async fn check_conflict(&self, path: &Path, is_conflict: bool) {
        let mut conflicts = self.conflicts.write().await;
        if is_conflict {
            conflicts.insert(path.to_owned());
        } else {
            conflicts.remove(path);
        }
    }

    async fn do_copy_or_mkdir<S, D>(
        &self,
        metadata: &fsync::Metadata,
        src: &S,
        dest: &D,
        dir: StorageDir,
    ) -> fsync::Result<()>
    where
        S: storage::Storage,
        D: storage::Storage,
    {
        let path = metadata.path();
        debug_assert!(path.is_absolute() && !path.is_root());
        let is_conflict = if metadata.is_file() {
            let read = src.read_file(path.to_owned()).await?;

            // create parents
            dest.mkdir(path.parent().unwrap(), true).await?;
            let conflicts = self.tree.ensure_parents(path, dir.dest());
            for (path, is_conflict) in conflicts {
                self.check_conflict(&path, is_conflict).await;
            }

            let metadata = dest.create_file(&metadata, read).await.unwrap();
            self.tree
                .add_to_storage_check_conflict(path, metadata, dir.dest())
        } else {
            assert!(metadata.is_dir());

            dest.mkdir(path, false).await?;
            let metadata = Metadata::Directory {
                path: path.to_path_buf(),
                stat: None,
            };
            self.tree
                .add_to_storage_check_conflict(path, metadata, dir.dest())
        };
        self.check_conflict(path, is_conflict).await;
        Ok(())
    }

    async fn do_replace<S, D>(
        &self,
        metadata: &fsync::Metadata,
        src: &S,
        dest: &D,
        dir: StorageDir,
    ) -> fsync::Result<()>
    where
        S: storage::Storage,
        D: storage::Storage,
    {
        let path = metadata.path();

        let data = src.read_file(path.to_path_buf()).await?;
        let written = dest.write_file(metadata, data).await?;
        let is_conflict = self
            .tree
            .add_to_storage_check_conflict(path, written, dir.dest());
        self.check_conflict(path, is_conflict).await;
        Ok(())
    }
}

impl<L, R> Service<L, R> {
    pub fn local(&self) -> &L {
        &self.local
    }

    pub fn remote(&self) -> &R {
        &self.remote
    }

    pub async fn entry_node(&self, path: &Path) -> Result<Option<fsync::tree::EntryNode>, Error> {
        let path = Self::check_path(path)?;
        Ok(self.tree.entry(&path))
    }

    pub async fn conflicts(
        &self,
        start: Option<&Path>,
        max_len: usize,
    ) -> fsync::Result<Vec<fsync::tree::Entry>> {
        let start = start.map(Self::check_path).transpose()?;
        let conflicts = self.conflicts.read().await;
        let start_bound = start.map(Bound::Included).unwrap_or(Bound::Unbounded);
        let conflicts = conflicts
            .range((start_bound, Bound::Unbounded))
            .take(max_len)
            .map(|path| {
                self.tree
                    .entry(path)
                    .expect("conflict path should point to valid entry")
                    .into_entry()
            })
            .collect();
        Ok(conflicts)
    }
}

impl<L, R> Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    pub async fn copy_or_mkdir(&self, path: &Path, dir: StorageDir) -> Result<(), Error> {
        let node = self.check_node(path)?;
        match (node.entry(), dir) {
            (tree::Entry::Local(..), StorageDir::RemoteToLocal) => {
                Err(PathError::Only(path.to_owned(), Location::Local))?
            }
            (tree::Entry::Remote(..), StorageDir::LocalToRemote) => {
                Err(PathError::Only(path.to_owned(), Location::Remote))?
            }
            (tree::Entry::Local(metadata), StorageDir::LocalToRemote) => {
                self.do_copy_or_mkdir(metadata, &self.local, &self.remote, dir)
                    .await
            }
            (tree::Entry::Remote(metadata), StorageDir::RemoteToLocal) => {
                self.do_copy_or_mkdir(metadata, &self.remote, &self.local, dir)
                    .await
            }
            (tree::Entry::Sync { .. }, _) => {
                Err(PathError::Unexpected(path.to_owned(), Location::Both))?
            }
        }
    }

    pub async fn replace(&self, path: &Path, dir: StorageDir) -> fsync::Result<()> {
        let node = self.check_node(path)?;
        match (node.entry(), dir) {
            (tree::Entry::Sync { remote, .. }, StorageDir::RemoteToLocal) => {
                self.do_replace(remote, &self.remote, &self.local, dir)
                    .await
            }
            (tree::Entry::Sync { local, .. }, StorageDir::LocalToRemote) => {
                self.do_replace(local, &self.local, &self.remote, dir).await
            }
            (tree::Entry::Local(local), _) => Err(PathError::Unexpected(
                local.path().to_owned(),
                Location::Local,
            ))?,
            (tree::Entry::Remote(remote), _) => Err(PathError::Unexpected(
                remote.path().to_owned(),
                Location::Remote,
            ))?,
        }
    }

    pub async fn delete(&self, path: &Path, location: Location) -> fsync::Result<()> {
        let node = self.check_node(path)?;
        match (node.entry(), location) {
            (tree::Entry::Local(..), Location::Local) => {
                self.local().delete(path).await?;
                self.tree.remove(path);
            }
            (tree::Entry::Remote(..), Location::Remote) => {
                self.remote().delete(path).await?;
                self.tree.remove(path);
            }
            (tree::Entry::Sync { .. }, Location::Local) => {
                self.local().delete(path).await?;
                self.tree
                    .remove_from_storage(path, fsync::StorageLoc::Local);
            }
            (tree::Entry::Sync { .. }, Location::Remote) => {
                self.remote().delete(path).await?;
                self.tree
                    .remove_from_storage(path, fsync::StorageLoc::Remote);
            }
            (tree::Entry::Sync { .. }, Location::Both) => {
                self.local().delete(path).await?;
                self.remote().delete(path).await?;
                self.tree.remove(path);
            }
            _ => Err(fsync::PathError::NotFound(
                path.to_path_buf(),
                Some(location),
            ))?,
        }
        self.check_conflict(path, false).await;
        Ok(())
    }

    pub async fn operate(&self, operation: &Operation) -> fsync::Result<()> {
        match operation {
            Operation::Copy(path, dir) => self.copy_or_mkdir(path.as_ref(), *dir).await,
            Operation::Replace(path, dir) => self.replace(path.as_ref(), *dir).await,
            Operation::Delete(path, location) => self.delete(path.as_ref(), *location).await,
        }
    }
}

impl<L, R> crate::Shutdown for Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    async fn shutdown(&self) -> anyhow::Result<()> {
        log::info!("Shutting service down");
        {
            let abort_handle = self.abort_handle.read().await;
            if let Some(abort_handle) = &*abort_handle {
                abort_handle.abort();
            }
        }
        let fut1 = self.local.shutdown();
        let fut2 = self.remote.shutdown();
        tokio::try_join!(fut1, fut2)?;
        Ok(())
    }
}

async fn spawn(fut: impl Future<Output = ()> + Send + 'static) {
    tokio::spawn(fut);
}

#[derive(Clone, Debug)]
pub struct RpcService<L, R> {
    inner: Arc<Service<L, R>>,
}

impl<L, R> RpcService<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    pub async fn new(service: Arc<Service<L, R>>, abort_handle: AbortHandle) -> Self {
        debug_assert!(
            service.abort_handle.read().await.is_none(),
            "Cannot share Service among multiple RpcService"
        );
        *service.abort_handle.write().await = Some(abort_handle);
        Self { inner: service }
    }

    pub async fn start(
        &self,
        instance_name: &str,
        abort_reg: AbortRegistration,
    ) -> anyhow::Result<()> {
        let server_addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), 0);

        let mut listener =
            tarpc::serde_transport::tcp::listen(&server_addr, Bincode::default).await?;

        log::info!("Listening on port {}", listener.local_addr().port());

        let port_path = inst::runtime_port_file(instance_name)?;
        tokio::fs::create_dir_all(port_path.parent().unwrap()).await?;

        let port_str = serde_json::to_string(&listener.local_addr().port())?;
        log::trace!("Creating file {port_path}");
        tokio::fs::write(&port_path, port_str.as_bytes()).await?;

        listener.config_mut().max_frame_length(usize::MAX);
        let fut = listener
            // Ignore accept errors.
            .filter_map(|r| future::ready(r.ok()))
            .map(server::BaseChannel::with_defaults)
            // Limit channels to 1 per IP.
            .max_channels_per_key(1, |t| t.transport().peer_addr().unwrap().ip())
            // serve is generated by the service attribute. It takes as input any type implementing
            // the generated Fsync trait.
            .map(|channel| channel.execute(self.clone().serve()).for_each(spawn))
            // Max 10 channels.
            .buffer_unordered(10)
            .for_each(|_| async {});

        let _ = Abortable::new(fut, abort_reg).await;

        log::trace!("Removing file {port_path}");
        tokio::fs::remove_file(&port_path).await?;
        Ok(())
    }
}

impl<L, R> Fsync for RpcService<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    async fn conflicts(
        self,
        _: Context,
        start: Option<PathBuf>,
        max_len: u32,
    ) -> fsync::Result<Vec<fsync::tree::Entry>> {
        let max_len = max_len.min(100);
        let res = self.inner.conflicts(start.as_deref(), max_len as _).await;
        log::trace!(target: "RPC", "Fsync::conflicts({start:?}, {max_len}) -> {res:#?}");
        res
    }

    async fn entry_node(
        self,
        _: Context,
        path: PathBuf,
    ) -> fsync::Result<Option<fsync::tree::EntryNode>> {
        let res = self.inner.entry_node(&path).await;
        log::trace!(target: "RPC", "Fsync::entry(path: {path:?}) -> {res:#?}");
        res
    }

    async fn operate(self, _: Context, action: fsync::Operation) -> fsync::Result<OpRes> {
        let res = self.inner.operate(&action).await;
        log::trace!(target: "RPC", "{action:#?} -> {res:#?}");
        res.map(|()| OpRes::Done)
    }

    async fn progress(self, _: Context, path: PathBuf) -> fsync::Result<Option<fsync::Progress>> {
        let res = Ok(None);
        log::trace!(target: "RPC", "Fsync::progress(path: {path:?}) -> {res:#?}");
        res
    }

    async fn all_progress(self, _: Context) -> fsync::Result<Vec<(PathBuf, fsync::Progress)>> {
        let res = Ok(vec![]);
        log::trace!(target: "RPC", "Fsync::all_progress() -> {res:#?}");
        res
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, ops::Bound};

    use fsync::path::{Path, PathBuf};

    fn build_test_conflicts() -> BTreeSet<PathBuf> {
        let mut set = BTreeSet::new();

        set.insert(PathBuf::from("/a/a/a"));
        set.insert(PathBuf::from("/a/a/b"));
        set.insert(PathBuf::from("/a/b/a"));
        set.insert(PathBuf::from("/a/b/b"));

        set.insert(PathBuf::from("/b/a/a"));
        set.insert(PathBuf::from("/b/a/b"));
        set.insert(PathBuf::from("/b/b/a"));
        set.insert(PathBuf::from("/b/b/b"));

        set.insert(PathBuf::from("/c/a/a"));
        set.insert(PathBuf::from("/c/a/b"));
        set.insert(PathBuf::from("/c/b/a"));
        set.insert(PathBuf::from("/c/b/b"));

        set
    }

    fn conflicts_of(conflicts: &BTreeSet<PathBuf>, path: &Path) -> Vec<PathBuf> {
        let mut res = Vec::new();

        let start = Bound::Included(path.to_owned());
        let end = Bound::Unbounded;

        let mut conf = conflicts.range((start, end));
        while let Some(entry) = conf.next() {
            if entry.as_str().starts_with(path.as_str()) {
                res.push(entry.clone());
            } else {
                break;
            }
        }

        res
    }

    #[test]
    fn test_dir_contain_conflict() {
        let conflicts = build_test_conflicts();

        let bs = conflicts_of(&conflicts, Path::new("/b"));

        assert_eq!(bs.len(), 4);
        assert_eq!(bs[0], PathBuf::from("/b/a/a"));
        assert_eq!(bs[1], PathBuf::from("/b/a/b"));
        assert_eq!(bs[2], PathBuf::from("/b/b/a"));
        assert_eq!(bs[3], PathBuf::from("/b/b/b"));
    }
}
