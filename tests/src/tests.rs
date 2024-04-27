use fsync::{
    path::{Path, PathBuf}, stat, tree::Entry, DeletionMethod, Operation, ResolutionMethod
};

use crate::{
    dataset::{self, Dataset, Patch},
    harness, 
    utils::UnwrapDisplay,
};

#[tokio::test]
async fn entry() {
    let h = harness().await;
    let notexist = h
        .service
        .entry_node(Path::new("/not-exists"))
        .await
        .unwrap();
    assert!(notexist.is_none());
    let exist = h.service.entry_node(Path::new("/both.txt")).await.unwrap();
    match exist {
        None => unreachable!(),
        Some(exist) => {
            assert!(exist.entry().is_sync());
        }
    }
}

#[tokio::test]
async fn node_stat() {
    let h = harness().await;
    let root = h.service.entry_node(Path::root()).await.unwrap().unwrap();
    let stat = root.stats();
    assert_eq!(stat.node, dataset::DEFAULT_NODE_STAT);
}

#[tokio::test]
async fn sync_remote_to_local() {
    let h = harness().await;

    let root = h.service.entry_node(Path::root()).await.unwrap().unwrap();
    let orig_stat = root.stats();

    let path = PathBuf::from("/only-remote.txt");
    h.service
        .clone()
        .operate(Operation::Sync(path.clone()))
        .await
        .unwrap();

    let content = h.local_file_content(&path).await.unwrap();
    assert_eq!(&content, path.as_str());

    let added_stat = stat::Tree {
        local: stat::Dir {
            data: content.len() as i64,
            dirs: 0,
            files: 1,
        },
        remote: stat::Dir::null(),
        node: stat::Node {
            nodes: 0,
            sync: 1,
            conflicts: 0,
        },
    };

    let root = h.service.entry_node(Path::root()).await.unwrap().unwrap();
    let new_stat = root.stats();
    assert_eq!(new_stat, orig_stat + added_stat);
}

#[tokio::test]
async fn sync_remote_to_local_deep() {
    let h = harness().await;
    let path = PathBuf::from("/only-remote/deep/file2.txt");
    h.service
        .clone()
        .operate(Operation::Sync(path.clone()))
        .await
        .unwrap();
    let content = h.local_file_content(&path).await.unwrap();
    assert_eq!(&content, path.as_str());
    let deep = PathBuf::from("/only-remote/deep");
    let deep_node = h
        .service
        .entry_node(&deep)
        .await
        .unwrap()
        .expect("should have the deep dir entry");
    assert!(matches!(deep_node.entry(), Entry::Sync { .. }));
}

#[tokio::test]
#[should_panic(expected = "No such entry: /not-a-file.txt")]
async fn sync_remote_to_local_fail_missing() {
    let h = harness().await;
    let path = PathBuf::from("/not-a-file.txt");
    h.service
        .clone()
        .operate(Operation::Sync(path))
        .await
        .unwrap_display();
}

#[tokio::test]
async fn sync_local_to_remote_deep() {
    let h = harness().await;
    let path = PathBuf::from("/only-local/deep/file2.txt");
    h.service
        .clone()
        .operate(Operation::Sync(path.clone()))
        .await
        .unwrap();
    let content = h.remote_file_content(&path).await.unwrap();
    assert_eq!(&content, path.as_str());
    let deep = PathBuf::from("/only-local/deep");
    let deep_node = h
        .service
        .entry_node(&deep)
        .await
        .unwrap()
        .expect("should have the deep dir entry");
    assert!(matches!(deep_node.entry(), Entry::Sync { .. }));
}

#[tokio::test]
#[should_panic(expected = "Expected an absolute path: only-remote.txt")]
async fn sync_remote_to_local_fail_relative() {
    let h = harness().await;
    let path = PathBuf::from("only-remote.txt");
    h.service
        .clone()
        .operate(Operation::Sync(path))
        .await
        .unwrap_display();
}

#[tokio::test]
async fn sync_local_to_remote() {
    let h = harness().await;
    let path = PathBuf::from("/only-local.txt");
    h.service
        .clone()
        .operate(Operation::Sync(path.clone()))
        .await
        .unwrap();
    let content = h.remote_file_content(&path).await.unwrap();
    assert_eq!(&content, path.as_str());
}

#[tokio::test]
#[should_panic(expected = "No such entry: /not-a-file.txt")]
async fn sync_local_to_remote_fail_missing() {
    let h = harness().await;
    let path = PathBuf::from("/not-a-file.txt");
    h.service
        .clone()
        .operate(Operation::Sync(path))
        .await
        .unwrap_display();
}

#[tokio::test]
async fn resolve_keep_newer_local() {
    let h = Dataset::default()
        .with_mtime_now()
        .apply_remote(Patch::Age("/conflict.txt".into(), 10))
        .harness()
        .await;
    let path = PathBuf::from("/conflict.txt");
    h.service
        .clone()
        .operate(Operation::Resolve(
            path.clone(),
            ResolutionMethod::ReplaceOlderByNewer,
        ))
        .await
        .unwrap();
    let local_content = h.local_file_content(&path).await.unwrap();
    let remote_content = h.remote_file_content(&path).await.unwrap();
    assert_eq!(&local_content, "/conflict.txt - local");
    assert_eq!(&remote_content, "/conflict.txt - local");
}

#[tokio::test]
async fn resolve_keep_newer_remote() {
    let h = Dataset::default()
        .with_mtime_now()
        .apply_local(Patch::Age("/conflict.txt".into(), 10))
        .harness()
        .await;
    let path = PathBuf::from("/conflict.txt");
    h.service
        .clone()
        .operate(Operation::Resolve(
            path.clone(),
            ResolutionMethod::ReplaceOlderByNewer,
        ))
        .await
        .unwrap();
    let local_content = h.local_file_content(&path).await.unwrap();
    let remote_content = h.remote_file_content(&path).await.unwrap();
    assert_eq!(&local_content, "/conflict.txt - remote");
    assert_eq!(&remote_content, "/conflict.txt - remote");
}

#[tokio::test]
async fn delete_local() {
    let h = harness().await;
    let path = PathBuf::from("/both.txt");
    h.service
        .clone()
        .operate(Operation::Delete(path.clone(), DeletionMethod::Local))
        .await
        .unwrap();
    let node = h.service.entry_node(&path).await.unwrap();
    assert!(node.unwrap().is_remote_only());
}

#[tokio::test]
async fn delete_remote() {
    let h = harness().await;
    let path = PathBuf::from("/both.txt");
    h.service
        .clone()
        .operate(Operation::Delete(path.clone(), DeletionMethod::Remote))
        .await
        .unwrap();
    let node = h.service.entry_node(&path).await.unwrap();
    assert!(node.unwrap().is_local_only());
}

#[tokio::test]
async fn delete_both() {
    let h = harness().await;
    let path = PathBuf::from("/both.txt");
    h.service
        .clone()
        .operate(Operation::Delete(path.clone(), DeletionMethod::All))
        .await
        .unwrap();
    let node = h.service.entry_node(&path).await.unwrap();
    assert!(node.is_none());
}
