#![cfg(test)]

use fsync::path::{FsPath, Path};
use fsyncd::{
    service::Service,
    storage::{
        cache::{CachePersist, CacheStorage},
        Storage,
    },
};
use futures::prelude::*;

//mod config;
mod utils;
mod stubs {
    //pub mod drive;
    pub mod fs;
    pub mod id;
}
mod tests;

use libtest_mimic::Failed;
use stubs::{fs, id};

pub struct Harness<L, R> {
    pub service: Service<L, R>,
}

impl<L, R> Harness<L, R>
where
    L: Storage,
    R: Storage,
{
    pub fn local(&self) -> &L {
        self.service.local()
    }

    pub fn remote(&self) -> &R {
        self.service.remote()
    }

    pub async fn local_file_content(&self, path: &Path) -> anyhow::Result<String> {
        let r = self.local().read_file(path.to_owned()).await?;
        let c = utils::file_content(r).await?;
        Ok(c)
    }
}

type FsHarness = Harness<fs::Stub, fs::Stub>;
type CacheHarness = Harness<fs::Stub, CacheStorage<id::Stub>>;

pub async fn make_fs_harness() -> FsHarness {
    let dir = FsPath::new(env!("CARGO_MANIFEST_DIR"));

    let local_dir = dir.join("local");
    let local = fs::Stub::new(&local_dir);

    let remote_dir = dir.join("remote");
    let remote = fs::Stub::new(&remote_dir);

    let (local, remote) = tokio::try_join!(local, remote).unwrap();

    let service = Service::new(local, remote).await.unwrap();

    Harness { service }
}

async fn make_cache_harness() -> CacheHarness {
    let dir = FsPath::new(env!("CARGO_MANIFEST_DIR"));

    let local_dir = dir.join("local");
    let local = fs::Stub::new(&local_dir);

    let remote_dir = dir.join("remote");
    let remote = id::Stub::new(&remote_dir).await.unwrap();
    let remote = CacheStorage::new(remote, CachePersist::Memory);

    let (local, remote) = tokio::try_join!(local, remote).unwrap();

    let service = Service::new(local, remote).await.unwrap();

    Harness { service }
}

fn trial_fs<F, Fut>(func: F) -> Result<(), Failed>
where
    F: FnOnce(FsHarness) -> Fut,
    Fut: Future<Output = Result<(), Failed>> + Send,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    rt.block_on(async move {
        let harness = make_fs_harness().await;
        func(harness).await
    })
}

fn trial_cache<F, Fut>(func: F) -> Result<(), Failed>
where
    F: FnOnce(CacheHarness) -> Fut,
    Fut: Future<Output = Result<(), Failed>> + Send,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    rt.block_on(async move {
        let harness = make_cache_harness().await;
        func(harness).await
    })
}

macro_rules! add_test {
    ($tests:expr, $func:ident) => {
        let name_fs = concat!(stringify!($func), " - fs");
        let name_cache = concat!(stringify!($func), " - cache");
        $tests.push(libtest_mimic::Trial::test(name_fs, || trial_fs($func)));
        $tests.push(libtest_mimic::Trial::test(name_cache, || {
            trial_cache($func)
        }));
    };
}

fn main() {
    use libtest_mimic::Arguments;
    use tests::*;

    let args = Arguments::from_args();
    let mut tests = Vec::new();

    add_test!(tests, copy_remote_to_local);
    add_test!(tests, copy_remote_to_local_fail_missing);
    add_test!(tests, copy_remote_to_local_fail_relative);

    libtest_mimic::run(&args, tests).exit();
}
