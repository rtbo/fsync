use fsync::path::{Path, PathBuf};

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
    h.service.copy_remote_to_local(&path).await.unwrap();
    let content = h.local_file_content(&path).await.unwrap();
    assert_eq!(&content, path.as_str());
}

#[tokio::test]
#[should_panic]
async fn copy_remote_to_local_fail_missing() {
    let h = harness().await;
    let path = PathBuf::from("/not-a-file.txt");
    h.service.copy_remote_to_local(&path).await.unwrap();
}

#[tokio::test]
#[should_panic]
async fn copy_remote_to_local_fail_relative() {
    let harness = harness().await;
    let path = PathBuf::from("only-remote.txt");
    harness.service.copy_remote_to_local(&path).await.unwrap();
}

#[tokio::test]
async fn copy_local_to_remote() {
    let h = harness().await;
    let path = PathBuf::from("/only-local.txt");
    h.service.copy_local_to_remote(&path).await.unwrap();
    let content = h.remote_file_content(&path).await.unwrap();
    assert_eq!(&content, path.as_str());
}

#[tokio::test]
#[should_panic]
async fn copy_local_to_remote_fail_missing() {
    let h = harness().await;
    let path = PathBuf::from("/not-a-file.txt");
    h.service.copy_local_to_remote(&path).await.unwrap();
}