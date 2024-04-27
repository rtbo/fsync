use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv6Addr},
    ops::Bound,
    sync::Arc,
    time::Duration,
};

use async_read_progress::TokioAsyncReadProgressExt;
use fsync::{
    self,
    loc::inst,
    path::{Path, PathBuf},
    stat,
    tree::EntryNode,
    DeletionMethod, Error, Fsync, Metadata, Operation, PathError, Progress, ResolutionMethod,
    StorageDir, StorageLoc,
};
use futures::{
    future::{self, BoxFuture},
    prelude::*,
    stream::{AbortHandle, AbortRegistration, Abortable},
};
use tarpc::{
    context::Context,
    server::{self, incoming::Incoming, Channel},
    tokio_serde::formats::Bincode,
};
use tokio::sync::{mpsc, RwLock};

use crate::{
    storage,
    tree::{self, DiffTree},
    SharedProgress,
};

#[derive(Debug)]
pub struct Service<L, R> {
    local: L,
    remote: R,
    tree: DiffTree,
    conflicts: RwLock<BTreeSet<PathBuf>>,
    abort_handle: RwLock<Option<AbortHandle>>,
    progresses: Arc<RwLock<Vec<(PathBuf, SharedProgress)>>>,
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
            progresses: Arc::new(RwLock::new(vec![])),
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

    async fn do_ensure_parents<S>(
        &self,
        path: &Path,
        storage: &S,
        loc: StorageLoc,
        progress: &SharedProgress,
    ) -> fsync::Result<()>
    where
        S: storage::MkDir,
    {
        // create parents
        storage
            .mkdir(path.parent().unwrap(), true, Some(progress))
            .await?;
        let conflicts = self.tree.ensure_parents(path, loc);

        for (path, is_conflict) in conflicts {
            self.check_conflict(&path, is_conflict).await;
        }

        Ok(())
    }

    async fn do_copy<S>(
        &self,
        metadata_from: &fsync::Metadata,
        to: &Path,
        storage: &S,
        loc: StorageLoc,
        progress: &SharedProgress,
    ) -> fsync::Result<()>
    where
        S: storage::MkDir + storage::CopyFile,
    {
        let path = metadata_from.path();
        debug_assert!(path.is_absolute() && !path.is_root());
        debug_assert!(metadata_from.is_file());

        self.do_ensure_parents(path, storage, loc, progress).await?;

        let metadata = storage
            .copy_file(metadata_from.path(), to, Some(progress))
            .await?;

        let is_conflict = self.tree.add_to_storage_check_conflict(path, metadata, loc);

        self.check_conflict(path, is_conflict).await;

        Ok(())
    }

    async fn do_clone<S, D>(
        &self,
        metadata: &fsync::Metadata,
        src: &S,
        dest: &D,
        dir: StorageDir,
        progress: &SharedProgress,
    ) -> fsync::Result<()>
    where
        S: storage::ReadFile,
        D: storage::MkDir + storage::CreateFile,
    {
        let path = metadata.path();
        debug_assert!(path.is_absolute() && !path.is_root());
        debug_assert!(metadata.is_file());
        let total = metadata.size().unwrap_or(0);
        let progress2 = progress.clone();

        let read = src.read_file(path.to_owned(), Some(progress)).await?;
        log::debug!("read {path}");
        let read = read.report_progress(Duration::from_millis(50), move |prog| {
            log::debug!("progress: {} of {}", prog, total);
            progress2.set(fsync::Progress::Progress {
                progress: prog as _,
                total,
            });
        });
        log::debug!("reporting progress on {path}");

        self.do_ensure_parents(path, dest, dir.dest(), progress)
            .await?;

        let metadata = dest
            .create_file(&metadata, read, Some(progress))
            .await
            .unwrap();
        let is_conflict = self
            .tree
            .add_to_storage_check_conflict(path, metadata, dir.dest());
        self.check_conflict(path, is_conflict).await;
        Ok(())
    }

    async fn do_mkdir<D>(
        &self,
        metadata: &fsync::Metadata,
        dest: &D,
        dir: StorageDir,
        progress: &SharedProgress,
    ) -> fsync::Result<()>
    where
        D: storage::MkDir,
    {
        let path = metadata.path();
        debug_assert!(path.is_absolute() && !path.is_root());
        debug_assert!(metadata.is_dir());

        dest.mkdir(path, false, Some(progress)).await?;
        let metadata = Metadata::Directory {
            path: path.to_path_buf(),
            stat: Some(stat::Dir::null()),
        };
        let is_conflict = self
            .tree
            .add_to_storage_check_conflict(path, metadata, dir.dest());
        self.check_conflict(path, is_conflict).await;
        Ok(())
    }

    async fn do_replace<S, D>(
        &self,
        metadata: &fsync::Metadata,
        src: &S,
        dest: &D,
        dir: StorageDir,
        progress: &SharedProgress,
    ) -> fsync::Result<()>
    where
        S: storage::ReadFile,
        D: storage::WriteFile,
    {
        let path = metadata.path();

        let total = metadata.size().unwrap_or(0);
        let data = src.read_file(path.to_path_buf(), Some(progress)).await?;
        let data = data.report_progress(Duration::from_millis(50), move |prog| {
            progress.set(fsync::Progress::Progress {
                progress: prog as _,
                total,
            });
        });
        let written = dest.write_file(metadata, data, Some(progress)).await?;
        let is_conflict = self
            .tree
            .add_to_storage_check_conflict(path, written, dir.dest());
        self.check_conflict(path, is_conflict).await;
        Ok(())
    }

    async fn do_delete<S>(
        &self,
        path: &Path,
        storage: &S,
        loc: StorageLoc,
        progress: &SharedProgress,
    ) -> fsync::Result<()>
    where
        S: storage::Delete,
    {
        storage.delete(path, Some(progress)).await?;
        self.tree.remove_from_storage(path, loc);
        self.check_conflict(path, false).await;
        Ok(())
    }
}

impl<L, R> Service<L, R>
where
    L: 'static,
    R: 'static,
{
    /// Poll progresses until all progresses are done.
    /// The progresses are polled every 100ms.
    /// When a progress is done, it is removed from the list.
    /// The loop exits when the list is empty.
    async fn progress_poll_loop(progresses: Arc<RwLock<Vec<(PathBuf, SharedProgress)>>>) {
        log::trace!("Entering progress poll loop");
        let start = std::time::Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let mut progresses = progresses.write().await;
            let len = progresses.len();
            let mut removed = 0;
            for i in (0..len).rev() {
                if progresses[i].1.get().is_done() {
                    log::info!("operation on {} is done", progresses[i].0);
                    progresses.remove(i);
                    removed += 1;
                }
            }
            drop(progresses);

            if removed == len {
                break;
            }
        }
        log::trace!(
            "Exiting progress poll loop after {}µs",
            start.elapsed().as_micros()
        );
    }

    async fn add_progress(&self, path: PathBuf, progress: SharedProgress) {
        log::info!("Logging operation progress on {path}");
        let mut progresses = self.progresses.write().await;
        progresses.push((path, progress));
        if progresses.len() == 1 {
            tokio::spawn(Self::progress_poll_loop(self.progresses.clone()));
        }
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

    pub async fn progress(&self, path: &Path) -> fsync::Result<Option<fsync::Progress>> {
        let path = Self::check_path(path)?;
        let progress = self.progresses.read().await.iter().find_map(|(p, prog)| {
            if p == &path {
                Some(prog.get())
            } else {
                None
            }
        });
        Ok(progress)
    }

    pub async fn progresses(&self, path: &Path) -> fsync::Result<Vec<(PathBuf, fsync::Progress)>> {
        let progresses = self.progresses.read().await.clone();
        Ok(progresses
            .into_iter()
            .filter(|(p, _)| path == p || path.is_ancestor_of(p))
            .map(|(path, prog)| (path, prog.get()))
            .collect())
    }
}

async fn track_progress<F, Fut>(
    path: PathBuf,
    tx: mpsc::Sender<(PathBuf, SharedProgress)>,
    f: F,
) -> fsync::Result<()>
where
    F: FnOnce(SharedProgress) -> Fut,
    Fut: Future<Output = fsync::Result<()>> + Send,
{
    let progress = SharedProgress::new();

    tx.send((path, progress.clone()))
        .await
        .expect("tx should not be closed");

    let res = f(progress.clone()).await;

    match res {
        Ok(()) => {
            progress.set(Progress::Done);
            Ok(())
        }
        Err(err) => {
            progress.set(Progress::Err(err.clone()));
            Err(err)
        }
    }
}

impl<L, R> Service<L, R>
where
    L: storage::Storage,
    R: storage::Storage,
{
    async fn sync_unit(
        &self,
        path: &Path,
        node: &EntryNode,
        progress: &SharedProgress,
    ) -> Result<(), Error> {
        match node.entry() {
            tree::Entry::Local(metadata) => {
                if metadata.is_dir() {
                    self.do_mkdir(metadata, &self.remote, StorageDir::LocalToRemote, progress)
                        .await
                } else {
                    self.do_clone(
                        metadata,
                        &self.local,
                        &self.remote,
                        StorageDir::LocalToRemote,
                        progress,
                    )
                    .await
                }
            }
            tree::Entry::Remote(metadata) => {
                if metadata.is_dir() {
                    self.do_mkdir(metadata, &self.local, StorageDir::RemoteToLocal, progress)
                        .await
                } else {
                    self.do_clone(
                        metadata,
                        &self.remote,
                        &self.local,
                        StorageDir::RemoteToLocal,
                        progress,
                    )
                    .await
                }
            }
            tree::Entry::Sync { conflict: None, .. } => Ok(()),
            tree::Entry::Sync { .. } => Err(fsync::Error::Conflict(path.to_owned())),
        }
    }

    async fn resolve_unit(
        &self,
        path: &Path,
        node: &EntryNode,
        method: ResolutionMethod,
        progress: &SharedProgress,
    ) -> Result<(), Error> {
        match node.entry() {
            tree::Entry::Sync {
                local,
                remote,
                conflict: Some(conflict),
            } => match (method, conflict) {
                (ResolutionMethod::DeleteRemote, _)
                | (ResolutionMethod::DeleteOlder, fsync::Conflict::LocalNewer)
                | (ResolutionMethod::DeleteNewer, fsync::Conflict::LocalOlder) => {
                    self.do_delete(path, &self.remote, StorageLoc::Remote, progress)
                        .await
                }
                (ResolutionMethod::DeleteLocal, _)
                | (ResolutionMethod::DeleteOlder, fsync::Conflict::LocalOlder)
                | (ResolutionMethod::DeleteNewer, fsync::Conflict::LocalNewer) => {
                    self.do_delete(path, &self.local, StorageLoc::Local, progress)
                        .await
                }
                (ResolutionMethod::ReplaceRemoteByLocal, _)
                | (ResolutionMethod::ReplaceOlderByNewer, fsync::Conflict::LocalNewer)
                | (ResolutionMethod::ReplaceNewerByOlder, fsync::Conflict::LocalOlder) => {
                    self.do_replace(
                        local,
                        &self.local,
                        &self.remote,
                        StorageDir::LocalToRemote,
                        progress,
                    )
                    .await
                }
                (ResolutionMethod::ReplaceLocalByRemote, _)
                | (ResolutionMethod::ReplaceOlderByNewer, fsync::Conflict::LocalOlder)
                | (ResolutionMethod::ReplaceNewerByOlder, fsync::Conflict::LocalNewer) => {
                    self.do_replace(
                        remote,
                        &self.remote,
                        &self.local,
                        StorageDir::RemoteToLocal,
                        progress,
                    )
                    .await
                }
                (ResolutionMethod::CreateLocalCopy, _) => {
                    self.do_copy(
                        local,
                        &copy_path(path),
                        &self.local,
                        StorageLoc::Local,
                        progress,
                    )
                    .await?;
                    self.do_replace(
                        remote,
                        &self.remote,
                        &self.local,
                        StorageDir::RemoteToLocal,
                        progress,
                    )
                    .await
                }
                (_, fsync::Conflict::LocalBigger) | (_, fsync::Conflict::LocalSmaller) => {
                    Err(fsync::Error::Unresolved(
                        path.to_owned(),
                        "local and remote have same mtime but different size. ".to_string(),
                    ))
                }
                (_, fsync::Conflict::LocalDirRemoteFile) => Err(fsync::Error::Unresolved(
                    path.to_owned(),
                    "local is dir and remote is file. ".to_string(),
                )),
                (_, fsync::Conflict::LocalFileRemoteDir) => Err(fsync::Error::Unresolved(
                    path.to_owned(),
                    "local is file and remote is dir. ".to_string(),
                )),
            },
            _ => Ok(()),
        }
    }

    async fn delete_unit(
        &self,
        path: &Path,
        node: &EntryNode,
        method: DeletionMethod,
        progress: &SharedProgress,
    ) -> fsync::Result<()> {
        if !node.children().is_empty() {
            return Err(fsync::Error::NotEmpty(path.to_owned()));
        }

        match (node.entry(), method) {
            // Delete only locally
            (tree::Entry::Local(..), DeletionMethod::Local | DeletionMethod::All)
            | (
                tree::Entry::Sync { conflict: None, .. },
                DeletionMethod::Local
                | DeletionMethod::LocalIfSync
                | DeletionMethod::LocalIfSyncNoConflict,
            )
            | (
                tree::Entry::Sync {
                    conflict: Some(_), ..
                },
                DeletionMethod::Local | DeletionMethod::LocalIfSync,
            ) => {
                self.do_delete(path, &self.local, StorageLoc::Local, progress)
                    .await
            }

            // Delete only remotely
            (tree::Entry::Remote(..), DeletionMethod::Remote | DeletionMethod::All)
            | (
                tree::Entry::Sync { conflict: None, .. },
                DeletionMethod::Remote
                | DeletionMethod::RemoteIfSync
                | DeletionMethod::RemoteIfSyncNoConflict,
            )
            | (
                tree::Entry::Sync {
                    conflict: Some(_), ..
                },
                DeletionMethod::Remote | DeletionMethod::RemoteIfSync,
            ) => {
                self.do_delete(path, &self.remote, StorageLoc::Remote, progress)
                    .await
            }

            // Delete everywhere
            (tree::Entry::Sync { .. }, DeletionMethod::All) => {
                let local = self.local().delete(path, Some(progress));
                let remote = self.remote().delete(path, Some(progress));
                futures::try_join!(local, remote)?;
                self.tree.remove(path);
                self.check_conflict(path, false).await;
                Ok(())
            }

            // Nothing to do
            (tree::Entry::Local(..), deletion) if deletion.is_remote() => Ok(()),
            (tree::Entry::Remote(..), deletion) if deletion.is_local() => Ok(()),

            // Conflict error
            (
                tree::Entry::Sync {
                    conflict: Some(_), ..
                },
                deletion,
            ) if deletion.no_conflict() => Err(fsync::Error::Conflict(path.to_owned())),

            (entry, method) => unreachable!("missing delete_unit case:{entry:#?}, {method:#?}"),
        }
    }

    async fn operate_unit(
        &self,
        operation: Operation,
        node: EntryNode,
        progress: SharedProgress,
    ) -> fsync::Result<()> {
        log::trace!("Operate unit: {operation:?}");
        match operation {
            Operation::Sync(path) => self.sync_unit(path.as_ref(), &node, &progress).await,
            Operation::Resolve(path, method) => {
                self.resolve_unit(path.as_ref(), &node, method, &progress)
                    .await
            }
            Operation::Delete(path, method) => {
                self.delete_unit(path.as_ref(), &node, method, &progress)
                    .await
            }
            _ => panic!("Not a unit operation: {operation:?}"),
        }
    }

    fn operate_deep<'a>(
        self: Arc<Self>,
        operation: Operation,
        node: EntryNode,
        progress: SharedProgress,
        tx: mpsc::Sender<(PathBuf, SharedProgress)>,
    ) -> BoxFuture<'a, fsync::Result<()>> {
        Box::pin(async move {
            log::trace!("Operate deep: {operation:?}");
            progress.set(Progress::Compound);

            let path = operation.path();

            let parent_first = matches!(
                operation,
                Operation::SyncDeep(..) | Operation::ResolveDeep(..)
            );
            if parent_first {
                self.operate_unit(operation.clone().not_deep(), node.clone(), progress.clone())
                    .await?;
            }

            let mut joinvec = Vec::new();
            for child_name in node.children() {
                let child_path = path.join(child_name);
                let child_node = self.check_node(path)?;
                let child_op = operation.with_path(child_path.clone());
                let this = self.clone();
                let tx2 = tx.clone();
                joinvec.push(track_progress(child_path, tx.clone(), |progress| async {
                    this.operate_deep(child_op, child_node, progress, tx2).await
                }));
            }
            future::try_join_all(joinvec).await?;

            if !parent_first {
                debug_assert!(matches!(operation, Operation::DeleteDeep(..)));
                self.operate_unit(operation.not_deep(), node.without_children(), progress)
                    .await?;
            }

            Ok(())
        })
    }

    pub async fn operate(self: Arc<Self>, operation: Operation) -> fsync::Result<Progress> {
        let (tx, mut rx) = mpsc::channel::<(PathBuf, SharedProgress)>(32);

        let join = {
            let this = self.clone();
            tokio::spawn(async move {
                let path = operation.path().to_owned();
                track_progress(path, tx.clone(), move |progress| async move {
                    let node = this.check_node(operation.path())?;
                    if operation.is_deep() {
                        this.operate_deep(operation, node, progress, tx).await
                    } else {
                        this.operate_unit(operation, node, progress).await
                    }
                })
                .await
            })
        };

        let sleep = tokio::time::sleep(Duration::from_millis(50));

        tokio::select! {
            res = join => {
                log::trace!("Operation completed within 50ms");
                match res {
                    Ok(Ok(())) => {
                        if cfg!(debug_assertions) {
                            let (_, prog) = rx.try_recv().expect("should receive at least root progress");
                            debug_assert!(matches!(prog.get(), Progress::Done));
                        }
                        Ok(Progress::Done)
                    },
                    Ok(Err(e)) => Err(e),
                    Err(err) => Err(fsync::Error::Bug(err.to_string())),
                }
            },
            _ = sleep => {
                log::trace!("Operation completed within 50ms");

                let (path, first_progress) = rx
                    .try_recv()
                    .expect("should receive at least root progress");

                self.add_progress(path, first_progress.clone()).await;

                tokio::spawn(async move {
                    while let Some((path, progress)) = rx.recv().await {
                        self.add_progress(path, progress).await;
                    }
                });
                Ok(first_progress.get())
            },
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

    async fn operate(self, _: Context, operation: fsync::Operation) -> fsync::Result<Progress> {
        if log::log_enabled!(log::Level::Trace) {
            let op = operation.clone();
            let res = self.inner.operate(operation).await;
            log::trace!(target: "RPC", "Fsync::operate({op:?}) -> {res:#?}");
            res
        } else {
            self.inner.operate(operation).await
        }
    }

    async fn progress(self, _: Context, path: PathBuf) -> fsync::Result<Option<fsync::Progress>> {
        let res = self.inner.progress(&path).await;
        log::trace!(target: "RPC", "Fsync::progress(path: {path:?}) -> {res:#?}");
        res
    }

    async fn progresses(
        self,
        _: Context,
        path: PathBuf,
    ) -> fsync::Result<Vec<(PathBuf, fsync::Progress)>> {
        let res = self.inner.progresses(&path).await;
        log::trace!(target: "RPC", "Fsync::progresses({path:#?}) -> {res:#?}");
        res
    }
}

fn copy_path(path: &Path) -> PathBuf {
    debug_assert!(!path.is_root());
    let parent = path
        .parent()
        .expect("Expected path to be copied to have a parent");
    let stem = path
        .file_stem()
        .expect("Expected path to be copied to have a name");
    let ext = path.extension();

    let cap = parent.as_str().len() + stem.len() + ext.map_or(0, |e| e.len() + 1) + 6;

    let mut res = PathBuf::with_capacity(cap);
    res.push(parent);
    res.push(format!("{stem}-copy"));
    if let Some(ext) = ext {
        res.set_extension(ext);
    }
    res
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, ops::Bound};

    use fsync::path::{Path, PathBuf};

    #[test]
    fn test_copy_path() {
        use super::copy_path;

        assert_eq!(
            Path::new("/parent/stem-copy.ext"),
            copy_path(&Path::new("/parent/stem.ext"))
        );

        assert_eq!(
            Path::new("/parent/noext-copy"),
            copy_path(&Path::new("/parent/noext"))
        );

        assert_eq!(
            Path::new("/file_at_root-copy.ext"),
            copy_path(&Path::new("/file_at_root.ext"))
        );
    }

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
