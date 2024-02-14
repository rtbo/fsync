use std::{cmp::Ordering, fmt, sync::Arc, time::Duration};

use byte_unit::{AdjustedByte, Byte, UnitType};
use fsync::{
    path::{Path, PathBuf},
    tree, Conflict, FsyncClient,
};
use futures::future::BoxFuture;
use inquire::Select;
use tokio::sync::RwLock;

use crate::utils;

fn tarpc_context() -> tarpc::context::Context {
    let mut ctx = tarpc::context::Context::current();
    ctx.deadline += Duration::from_secs(110);
    ctx
}

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Name of the fsyncd instance
    #[clap(long, short = 'n')]
    instance_name: Option<String>,

    /// Whether to recurse down the tree
    #[clap(long, short = 'r')]
    recurse: bool,

    /// Dry run only collects and prints the operations
    /// that would be performed on a regular run.
    #[clap(long, short = 'd')]
    dry_run: bool,

    /// Path of the entry to sync (root if not specified)
    path: Option<PathBuf>,
}

pub async fn main(args: Args) -> anyhow::Result<()> {
    let instance_name = match &args.instance_name {
        Some(name) => name.clone(),
        None => {
            let name = utils::single_instance_name()?;
            if let Some(name) = name {
                name
            } else {
                anyhow::bail!("Could not find a single share, please specify --share-name command line argument");
            }
        }
    };

    let client = utils::instance_client(&instance_name).await?;

    let path = args.path.clone().unwrap_or_else(PathBuf::root);
    let node = client
        .entry_node(tarpc_context(), path.clone())
        .await?
        .unwrap();
    if node.is_none() {
        anyhow::bail!("No such entry: {path}",);
    }
    let node = node.unwrap();
    let cmd = SyncCommand {
        client,
        args,
        remember: RwLock::default(),
        stats: RwLock::default(),
    };
    cmd.node(&node).await?;

    println!();
    if cmd.args.dry_run {
        println!("DRY RUN STATS");
    } else {
        println!("SYNC STATS");
    }
    let report = cmd.stat_report().await;
    println!();
    report.print_out();

    Ok(())
}

#[derive(Debug, Clone)]
enum Stat {
    Ignored {
        local: fsync::Metadata,
        remote: fsync::Metadata,
    },
    CopyRemoteToLocal(fsync::Metadata),
    CopyLocalToRemote(fsync::Metadata),
    ReplaceLocalByRemote {
        local: fsync::Metadata,
        remote: fsync::Metadata,
    },
    ReplaceRemoteByLocal {
        local: fsync::Metadata,
        remote: fsync::Metadata,
    },
    DeleteLocal(fsync::Metadata),
    GoodToGo(fsync::Metadata),
}

#[derive(Debug, Clone, Default)]
struct StatReport {
    local_files: i32,
    local_data: u64,
    downloaded_files: i32,
    downloaded_data: u64,

    remote_files: i32,
    remote_data: u64,
    uploaded_files: i32,
    uploaded_data: u64,

    replaced_locally: i32,
    replaced_remotely: i32,
    deleted_locally: i32,

    local_net_usage: i64,
    remote_net_usage: i64,
}

impl StatReport {
    fn calculate_new(stats: &[Stat]) -> Self {
        let mut report = Self::default();

        for stat in stats.iter() {
            match stat {
                Stat::Ignored { local, remote } => {
                    report.count_local(local);
                    report.count_remote(remote);
                }
                Stat::CopyRemoteToLocal(entry) => {
                    report.count_local(entry);
                    report.count_remote(entry);
                    report.add_local(entry);
                }
                Stat::CopyLocalToRemote(entry) => {
                    report.count_local(entry);
                    report.count_remote(entry);
                    report.add_remote(entry);
                }
                Stat::ReplaceLocalByRemote { local, remote } => {
                    report.count_local(remote);
                    report.count_remote(remote);
                    report.add_local(remote);
                    if local.is_file() {
                        report.replaced_locally += 1;
                    }
                    if let Some(size) = local.size() {
                        report.local_net_usage -= size as i64;
                    }
                }
                Stat::ReplaceRemoteByLocal { local, remote } => {
                    report.count_local(local);
                    report.count_remote(local);
                    report.add_remote(local);
                    if remote.is_file() {
                        report.replaced_remotely += 1;
                    }
                    if let Some(size) = remote.size() {
                        report.local_net_usage -= size as i64;
                    }
                }
                Stat::DeleteLocal(entry) => {
                    if entry.is_file() {
                        report.deleted_locally += 1;
                    }
                    if let Some(size) = entry.size() {
                        report.local_net_usage -= size as i64;
                    }
                }
                Stat::GoodToGo(entry) => {
                    report.count_local(entry);
                    report.count_remote(entry);
                }
            }
        }

        report
    }

    fn count_local(&mut self, local: &fsync::Metadata) {
        if local.is_file() {
            self.local_files += 1;
        }
        if let Some(size) = local.size() {
            self.local_data += size;
        }
    }

    fn count_remote(&mut self, remote: &fsync::Metadata) {
        if remote.is_file() {
            self.remote_files += 1;
        }
        if let Some(size) = remote.size() {
            self.remote_data += size;
        }
    }

    fn add_local(&mut self, entry: &fsync::Metadata) {
        if entry.is_file() {
            self.downloaded_files += 1;
        }
        if let Some(size) = entry.size() {
            self.downloaded_data += size;
            self.local_net_usage += size as i64;
        }
    }

    fn add_remote(&mut self, entry: &fsync::Metadata) {
        if entry.is_file() {
            self.uploaded_files += 1;
        }
        if let Some(size) = entry.size() {
            self.uploaded_data += size;
            self.remote_net_usage += size as i64;
        }
    }

    fn net_usage_sign_value(net_usage: i64) -> (&'static str, AdjustedByte) {
        let sign = if net_usage > 0 { "+" } else { "-" };
        let byte = utils::adjusted_byte(net_usage.unsigned_abs());
        (sign, byte)
    }

    fn print_out(&self) {
        let (sign, byte) = Self::net_usage_sign_value(self.local_net_usage);
        println!(
            "Downloaded {} files ({:#.2})",
            self.downloaded_files,
            utils::adjusted_byte(self.downloaded_data)
        );
        println!(
            "Uploaded {} files ({:#.2})",
            self.uploaded_files,
            utils::adjusted_byte(self.uploaded_data)
        );

        println!();

        println!(
            "Local drive: {} files ({:#.2} / {}{:#.2})",
            self.local_files,
            utils::adjusted_byte(self.local_data),
            sign,
            byte
        );
        let (sign, byte) = Self::net_usage_sign_value(self.remote_net_usage);
        println!(
            "Remote drive: {} files ({:#.2} / {}{:#.2})",
            self.remote_files,
            utils::adjusted_byte(self.remote_data),
            sign,
            byte
        );
        println!("{} files replaced locally", self.replaced_locally);
        println!("{} files replaced remotely", self.replaced_remotely);
        println!("{} files deleted locally", self.deleted_locally);
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum CopyChoice {
    Yes,
    No,
}

impl fmt::Display for CopyChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            CopyChoice::Yes => write!(f, "Yes"),
            CopyChoice::No => write!(f, "No"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ConflictChoice {
    ReplaceOldestByMostRecent,
    ReplaceLocalByRemote,
    ReplaceRemoteByLocal,
    DeleteLocal,
    Ignore,
}

impl fmt::Display for ConflictChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ConflictChoice::ReplaceOldestByMostRecent => write!(f, "Replace oldest by most recent"),
            ConflictChoice::ReplaceLocalByRemote => write!(f, "Replace local by remote (download)"),
            ConflictChoice::ReplaceRemoteByLocal => write!(f, "Replace remote by local (upload)"),
            ConflictChoice::DeleteLocal => write!(f, "Delete local (only keep remote)"),
            ConflictChoice::Ignore => write!(f, "Ignore"),
        }
    }
}

#[derive(Debug, Copy, Clone, Default)]
struct Remember {
    copy_remote_to_local: Option<CopyChoice>,
    copy_local_to_remote: Option<CopyChoice>,
    conflict: Option<ConflictChoice>,
}

#[derive(Debug, Copy, Clone)]
struct SelectOption<T: fmt::Display> {
    choice: T,
    remember: bool,
}

impl<T: fmt::Display> fmt::Display for SelectOption<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.remember {
            write!(f, "{} (remember for all)", self.choice)
        } else {
            write!(f, "{}", self.choice)
        }
    }
}

#[derive(Debug)]
struct SyncCommand {
    client: Arc<FsyncClient>,
    args: Args,
    remember: RwLock<Remember>,
    stats: RwLock<Vec<Stat>>,
}

impl SyncCommand {
    fn node<'a>(&'a self, node: &'a tree::EntryNode) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async {
            self.entry(node.entry()).await?;
            if self.args.recurse {
                let path = node.path();
                for c in node.children() {
                    let path = path.join(c);
                    let node = self
                        .client
                        .entry_node(tarpc_context(), path)
                        .await?
                        .unwrap();
                    self.node(node.as_ref().unwrap()).await?;
                }
            }
            Ok(())
        })
    }

    async fn entry(&self, entry: &tree::Entry) -> anyhow::Result<()> {
        match entry {
            tree::Entry::Local(entry) => self.local_to_remote(entry).await,
            tree::Entry::Remote(entry) => self.remote_to_local(entry).await,
            tree::Entry::Sync {
                local,
                remote,
                conflict,
            } => self.both(local, remote, *conflict).await,
        }
    }

    fn copy_choice_options(&self) -> Vec<SelectOption<CopyChoice>> {
        let mut options = vec![
            SelectOption {
                choice: CopyChoice::Yes,
                remember: false,
            },
            SelectOption {
                choice: CopyChoice::No,
                remember: false,
            },
        ];
        if self.args.recurse {
            options.extend_from_slice(&[
                SelectOption {
                    choice: CopyChoice::Yes,
                    remember: true,
                },
                SelectOption {
                    choice: CopyChoice::No,
                    remember: true,
                },
            ])
        }
        options
    }

    async fn local_to_remote(&self, entry: &fsync::Metadata) -> anyhow::Result<()> {
        let remember = {
            let rem = self.remember.read().await;
            rem.copy_local_to_remote
        };
        match remember {
            Some(CopyChoice::Yes) => {
                return self.copy_local_to_remote(entry).await;
            }
            Some(CopyChoice::No) => {
                return Ok(());
            }
            None => (),
        }

        let message = format!(
            "{} only exists locally. Do you wish to copy it on the remote drive?",
            entry.path()
        );
        let options = self.copy_choice_options();
        let ans =
            tokio::task::spawn_blocking(move || Select::new(&message, options).prompt_skippable());
        let ans = ans.await.unwrap()?;

        match ans {
            None => Ok(()),
            Some(SelectOption { choice, remember }) => {
                if remember {
                    let mut rem = self.remember.write().await;
                    rem.copy_local_to_remote = Some(choice);
                }
                if choice == CopyChoice::Yes {
                    self.copy_local_to_remote(entry).await
                } else {
                    Ok(())
                }
            }
        }
    }

    async fn remote_to_local(&self, entry: &fsync::Metadata) -> anyhow::Result<()> {
        let remember = {
            let rem = self.remember.read().await;
            rem.copy_remote_to_local
        };
        match remember {
            Some(CopyChoice::Yes) => {
                return self.copy_remote_to_local(entry).await;
            }
            Some(CopyChoice::No) => {
                return Ok(());
            }
            None => (),
        }

        let message = format!(
            "{} only exists remotely. Do you wish to copy it on the local drive?",
            entry.path()
        );
        let options = self.copy_choice_options();
        let ans =
            tokio::task::spawn_blocking(move || Select::new(&message, options).prompt_skippable());
        let ans = ans.await.unwrap()?;

        match ans {
            None => Ok(()),
            Some(SelectOption { choice, remember }) => {
                if remember {
                    let mut rem = self.remember.write().await;
                    rem.copy_remote_to_local = Some(choice);
                }
                if choice == CopyChoice::Yes {
                    self.copy_remote_to_local(entry).await
                } else {
                    Ok(())
                }
            }
        }
    }

    async fn both(
        &self,
        local: &fsync::Metadata,
        remote: &fsync::Metadata,
        conflict: Option<Conflict>,
    ) -> anyhow::Result<()> {
        assert_eq!(local.path(), remote.path());
        match (local, remote) {
            (fsync::Metadata::Special { path, .. }, _)
            | (_, fsync::Metadata::Special { path, .. }) => self.special_file(path).await,

            (fsync::Metadata::Symlink { .. }, _) | (_, fsync::Metadata::Symlink { .. }) => {
                unimplemented!("sync symlink")
            }

            (fsync::Metadata::Directory { .. }, fsync::Metadata::Directory { .. }) => {
                if !self.args.recurse {
                    println!(
                        concat!(
                            "{} is a directory. Nothing to do.\n",
                            "Specify the --recurse flag to recurse down the tree."
                        ),
                        local.path(),
                    );
                }
                Ok(())
            }

            (fsync::Metadata::Directory { .. }, _) => {
                self.local_dir_remote_file(local, remote).await
            }

            (_, fsync::Metadata::Directory { .. }) => {
                self.local_file_remote_dir(local, remote).await
            }

            (_, _) => self.both_reg_files(local, remote, conflict).await,
        }
    }

    async fn special_file(&self, path: &Path) -> anyhow::Result<()> {
        let message = format!("{path}: Unsupported special file (block, socket...).",);
        let options = vec!["Interrupt", "Ignore"];
        let ans = tokio::task::spawn_blocking(move || Select::new(&message, options).prompt());
        let ans = ans.await.unwrap()?;
        if ans == "Interrupt" {
            anyhow::bail!("Interrupted");
        }
        Ok(())
    }

    async fn local_dir_remote_file(
        &self,
        local: &fsync::Metadata,
        _remote: &fsync::Metadata,
    ) -> anyhow::Result<()> {
        anyhow::bail!(
            r#"{} isn't directory or file on both ends.
    Unsupported situation. Aborting"#,
            local.path()
        );
    }

    async fn local_file_remote_dir(
        &self,
        local: &fsync::Metadata,
        _remote: &fsync::Metadata,
    ) -> anyhow::Result<()> {
        anyhow::bail!(
            r#"{} isn't directory or file on both ends.
    Unsupported situation. Aborting"#,
            local.path()
        );
    }

    fn conflict_choice_options(&self) -> Vec<SelectOption<ConflictChoice>> {
        let remember = false;
        let mut options = vec![
            SelectOption {
                choice: ConflictChoice::ReplaceOldestByMostRecent,
                remember,
            },
            SelectOption {
                choice: ConflictChoice::ReplaceLocalByRemote,
                remember,
            },
            SelectOption {
                choice: ConflictChoice::ReplaceRemoteByLocal,
                remember,
            },
            SelectOption {
                choice: ConflictChoice::DeleteLocal,
                remember,
            },
            SelectOption {
                choice: ConflictChoice::Ignore,
                remember,
            },
        ];
        if self.args.recurse {
            let remember = true;
            options.extend_from_slice(&[
                SelectOption {
                    choice: ConflictChoice::ReplaceOldestByMostRecent,
                    remember,
                },
                SelectOption {
                    choice: ConflictChoice::ReplaceLocalByRemote,
                    remember,
                },
                SelectOption {
                    choice: ConflictChoice::ReplaceRemoteByLocal,
                    remember,
                },
                SelectOption {
                    choice: ConflictChoice::DeleteLocal,
                    remember,
                },
                SelectOption {
                    choice: ConflictChoice::Ignore,
                    remember,
                },
            ]);
        }
        options
    }

    async fn both_reg_files(
        &self,
        local: &fsync::Metadata,
        remote: &fsync::Metadata,
        conflict: Option<Conflict>,
    ) -> anyhow::Result<()> {
        if conflict.is_none() {
            println!("Up-to-date: {}", local.path());
            self.good_to_go(local).await;
            return Ok(());
        }

        match conflict {
            None => {
                println!("Up-to-date: {}", local.path());
                self.good_to_go(local).await;
                Ok(())
            }
            Some(Conflict::LocalBigger) | Some(Conflict::LocalSmaller) => {
                anyhow::bail!(
                    r#"{} has same modification time but different size.
    Unsupported situation. Aborting"#,
                    local.path()
                );
            }
            Some(Conflict::LocalDirRemoteFile) | Some(Conflict::LocalFileRemoteDir) => {
                unreachable!()
            }
            Some(Conflict::LocalNewer) | Some(Conflict::LocalOlder) => {
                self.conflict(local, remote).await
            }
        }
    }

    async fn conflict(
        &self,
        local: &fsync::Metadata,
        remote: &fsync::Metadata,
    ) -> anyhow::Result<()> {
        let loc_mtime = local.mtime().unwrap();
        let loc_size = local.size().unwrap();
        let rem_mtime = remote.mtime().unwrap();
        let rem_size = remote.size().unwrap();

        let mtime_cmp = fsync::compare_mtime(loc_mtime, rem_mtime);

        let remember = {
            let rem = self.remember.read().await;
            rem.conflict
        };

        if let Some(choice) = remember {
            return self.execute_conflict_choice(choice, local, remote).await;
        }

        let head = format!("Conflict: {}", local.path());
        let (loc_adj, rem_adj) = match mtime_cmp {
            Ordering::Less => ("oldest", "most recent"),
            Ordering::Greater => ("most recent", "oldest"),
            Ordering::Equal => unreachable!(),
        };

        let mtime = format!("Local is {loc_adj} ({loc_mtime}), remote is {rem_adj} ({rem_mtime})");
        let size: String = {
            let loc_bytes = utils::adjusted_byte(loc_size);
            let rem_bytes = utils::adjusted_byte(rem_size);

            match loc_size.cmp(&rem_size) {
                Ordering::Equal => "Both have same size".into(),
                Ordering::Less => {
                    format!("Local is smaller ({loc_bytes:#.2}), remote is bigger ({rem_bytes:#.2})")
                }
                Ordering::Greater => {
                    format!("Local is bigger ({loc_bytes:#.2}), remote is smaller ({rem_bytes:#.2})")
                }
            }
        };
        let message = format!("{head}\n{mtime}\n{size}");
        let options = self.conflict_choice_options();
        let ans = tokio::task::spawn_blocking(move || Select::new(&message, options).prompt());
        let SelectOption { choice, remember } = ans.await.unwrap()?;
        if remember {
            let mut rem = self.remember.write().await;
            rem.conflict = Some(choice);
        }
        self.execute_conflict_choice(choice, local, remote).await
    }

    async fn execute_conflict_choice(
        &self,
        choice: ConflictChoice,
        local: &fsync::Metadata,
        remote: &fsync::Metadata,
    ) -> anyhow::Result<()> {
        match choice {
            ConflictChoice::Ignore => self.ignore(local, remote).await,
            ConflictChoice::ReplaceOldestByMostRecent => {
                if local.mtime().unwrap() < remote.mtime().unwrap() {
                    self.replace_local_by_remote(local, remote).await
                } else {
                    self.replace_remote_by_local(local, remote).await
                }
            }
            ConflictChoice::ReplaceLocalByRemote => {
                self.replace_local_by_remote(local, remote).await
            }
            ConflictChoice::ReplaceRemoteByLocal => {
                self.replace_remote_by_local(local, remote).await
            }
            ConflictChoice::DeleteLocal => self.delete_local(local).await,
        }
    }
}

impl SyncCommand {
    async fn ignore(
        &self,
        local: &fsync::Metadata,
        remote: &fsync::Metadata,
    ) -> anyhow::Result<()> {
        {
            let mut stats = self.stats.write().await;
            stats.push(Stat::Ignored {
                local: local.clone(),
                remote: remote.clone(),
            });
        }

        Ok(())
    }

    async fn copy_remote_to_local(&self, entry: &fsync::Metadata) -> anyhow::Result<()> {
        match entry {
            fsync::Metadata::Directory { path } => {
                println!("creating local directory `{path}`");
            }
            fsync::Metadata::Regular { path, size, .. } => {
                let byte = Byte::from(*size).get_appropriate_unit(UnitType::Binary);
                println!(
                    "downloading `{}` in `{}` ({byte:#.2})",
                    path.file_name().unwrap(),
                    path.parent().unwrap(),
                );
            }
            _ => panic!("Unsupported {entry:?}"),
        }
        {
            let mut stats = self.stats.write().await;
            stats.push(Stat::CopyRemoteToLocal(entry.clone()));
        }

        if !self.args.dry_run {
            let operation = fsync::Operation::CopyRemoteToLocal(entry.path().to_owned());
            self.client.operate(tarpc_context(), operation).await??;
        }
        Ok(())
    }

    async fn copy_local_to_remote(&self, entry: &fsync::Metadata) -> anyhow::Result<()> {
        match entry {
            fsync::Metadata::Directory { path } => {
                println!("creating remote directory `{path}`");
            }
            fsync::Metadata::Regular { path, size, .. } => {
                let byte = Byte::from(*size).get_appropriate_unit(UnitType::Binary);
                println!(
                    "uploading `{}` in `{}` ({byte:#.2})",
                    path.file_name().unwrap(),
                    path.parent().unwrap(),
                );
            }
            _ => panic!("Unsupported {entry:?}"),
        }
        {
            let mut stats = self.stats.write().await;
            stats.push(Stat::CopyLocalToRemote(entry.clone()));
        }

        if !self.args.dry_run {
            let operation = fsync::Operation::CopyLocalToRemote(entry.path().to_owned());
            self.client.operate(tarpc_context(), operation).await??;
        }
        Ok(())
    }

    async fn good_to_go(&self, entry: &fsync::Metadata) {
        let mut stats = self.stats.write().await;
        stats.push(Stat::GoodToGo(entry.clone()));
    }

    async fn replace_local_by_remote(
        &self,
        local: &fsync::Metadata,
        remote: &fsync::Metadata,
    ) -> anyhow::Result<()> {
        assert_eq!(local.path(), remote.path());
        {
            let mut stats = self.stats.write().await;
            stats.push(Stat::ReplaceLocalByRemote {
                local: local.clone(),
                remote: remote.clone(),
            });
        }

        if !self.args.dry_run {
            let operation = fsync::Operation::ReplaceLocalByRemote(local.path().to_owned());
            self.client.operate(tarpc_context(), operation).await??;
        }
        Ok(())
    }

    async fn replace_remote_by_local(
        &self,
        local: &fsync::Metadata,
        remote: &fsync::Metadata,
    ) -> anyhow::Result<()> {
        assert_eq!(local.path(), remote.path());
        {
            let mut stats = self.stats.write().await;
            stats.push(Stat::ReplaceRemoteByLocal {
                local: local.clone(),
                remote: remote.clone(),
            });
        }

        if !self.args.dry_run {
            let operation = fsync::Operation::ReplaceRemoteByLocal(local.path().to_owned());
            self.client.operate(tarpc_context(), operation).await??;
        }
        Ok(())
    }

    async fn delete_local(&self, local: &fsync::Metadata) -> anyhow::Result<()> {
        {
            let mut stats = self.stats.write().await;
            stats.push(Stat::DeleteLocal(local.clone()));
        }

        if !self.args.dry_run {
            let operation =
                fsync::Operation::Delete(local.path().to_owned(), fsync::Location::Local);
            self.client.operate(tarpc_context(), operation).await??;
        }
        Ok(())
    }
}

impl SyncCommand {
    async fn stat_report(&self) -> StatReport {
        let stats = self.stats.read().await;
        StatReport::calculate_new(&stats)
    }
}
