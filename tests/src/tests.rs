use fsync::{
    path::{Path, PathBuf},
    Location, Operation,
};

use crate::{harness, utils::UnwrapDisplay};

#[tokio::test]
async fn entry() {
    let h = harness().await;
    let notexist = h.service.entry(Path::new("/not-exists")).await.unwrap();
    assert!(notexist.is_none());
    let exist = h.service.entry(Path::new("/both.txt")).await.unwrap();
    match exist {
        None => unreachable!(),
        Some(exist) => {
            assert!(exist.entry().is_both());
        }
    }
}

#[tokio::test]
async fn copy_remote_to_local() {
    let h = harness().await;
    let path = PathBuf::from("/only-remote.txt");
    h.service
        .operate(&Operation::CopyRemoteToLocal(path.clone()))
        .await
        .unwrap();
    let content = h.local_file_content(&path).await.unwrap();
    assert_eq!(&content, path.as_str());
}

#[tokio::test]
#[should_panic(expected = "No such entry: /not-a-file.txt")]
async fn copy_remote_to_local_fail_missing() {
    let h = harness().await;
    let path = PathBuf::from("/not-a-file.txt");
    h.service
        .operate(&Operation::CopyRemoteToLocal(path))
        .await
        .unwrap_display();
}

#[tokio::test]
#[should_panic(expected = "Expected an absolute path: only-remote.txt")]
async fn copy_remote_to_local_fail_relative() {
    let h = harness().await;
    let path = PathBuf::from("only-remote.txt");
    h.service
        .operate(&Operation::CopyRemoteToLocal(path))
        .await
        .unwrap_display();
}

#[tokio::test]
async fn copy_local_to_remote() {
    let h = harness().await;
    let path = PathBuf::from("/only-local.txt");
    h.service
        .operate(&Operation::CopyLocalToRemote(path.clone()))
        .await
        .unwrap();
    let content = h.remote_file_content(&path).await.unwrap();
    assert_eq!(&content, path.as_str());
}

#[tokio::test]
#[should_panic(expected = "No such entry: /not-a-file.txt")]
async fn copy_local_to_remote_fail_missing() {
    let h = harness().await;
    let path = PathBuf::from("/not-a-file.txt");
    h.service
        .operate(&Operation::CopyLocalToRemote(path))
        .await
        .unwrap_display();
}

#[tokio::test]
async fn replace_local_by_remote() {
    let h = harness().await;
    let path = PathBuf::from("/both.txt");
    h.service
        .operate(&Operation::ReplaceLocalByRemote(path.clone()))
        .await
        .unwrap();
    let local_content = h.local_file_content(&path).await.unwrap();
    let remote_content = h.remote_file_content(&path).await.unwrap();
    assert_eq!(&local_content, "/both.txt - remote");
    assert_eq!(&remote_content, "/both.txt - remote");
}

#[tokio::test]
async fn replace_remote_by_local() {
    let h = harness().await;
    let path = PathBuf::from("/both.txt");
    h.service
        .operate(&Operation::ReplaceRemoteByLocal(path.clone()))
        .await
        .unwrap();
    let local_content = h.local_file_content(&path).await.unwrap();
    let remote_content = h.remote_file_content(&path).await.unwrap();
    assert_eq!(&local_content, "/both.txt - local");
    assert_eq!(&remote_content, "/both.txt - local");
}

#[tokio::test]
async fn delete_local() {
    let h = harness().await;
    let path = PathBuf::from("/both.txt");
    h.service
        .operate(&Operation::Delete(path.clone(), Location::Local))
        .await
        .unwrap();
    let node = h.service.entry(&path).await.unwrap();
    assert!(node.unwrap().is_remote_only());
}

#[tokio::test]
async fn delete_remote() {
    let h = harness().await;
    let path = PathBuf::from("/both.txt");
    h.service
        .operate(&Operation::Delete(path.clone(), Location::Remote))
        .await
        .unwrap();
    let node = h.service.entry(&path).await.unwrap();
    assert!(node.unwrap().is_local_only());
}

#[tokio::test]
async fn delete_both() {
    let h = harness().await;
    let path = PathBuf::from("/both.txt");
    h.service
        .operate(&Operation::Delete(path.clone(), Location::Both))
        .await
        .unwrap();
    let node = h.service.entry(&path).await.unwrap();
    assert!(node.is_none());
}
