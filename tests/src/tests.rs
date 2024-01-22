use fsync::{
    path::{Path, PathBuf},
    Operation,
};

use crate::harness;

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
#[should_panic(expected = "not found")]
async fn copy_remote_to_local_fail_missing() {
    let h = harness().await;
    let path = PathBuf::from("/not-a-file.txt");
    h.service
        .operate(&Operation::CopyRemoteToLocal(path))
        .await
        .unwrap();
}

#[tokio::test]
#[should_panic(expected = "relative")]
async fn copy_remote_to_local_fail_relative() {
    let h = harness().await;
    let path = PathBuf::from("only-remote.txt");
    h.service
        .operate(&Operation::CopyRemoteToLocal(path))
        .await
        .unwrap();
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
#[should_panic(expected = "not found")]
async fn copy_local_to_remote_fail_missing() {
    let h = harness().await;
    let path = PathBuf::from("/not-a-file.txt");
    h.service
        .operate(&Operation::CopyLocalToRemote(path.clone()))
        .await
        .unwrap();
}
