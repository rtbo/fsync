use fsync::{
    path::{Path, PathBuf},
    stat,
    tree::Entry,
    Conflict, DeletionMethod, Operation, ResolutionMethod,
};

use crate::{
    dataset::{self, Dataset},
    harness,
    utils::UnwrapDisplay,
};

#[tokio::test]
async fn entry() {
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file("/file.txt", "Test content")],
            remote: vec![Entry::txt_file("/file.txt", "Test content")],
        })
        .await
    };

    let notexist = h
        .service
        .entry_node(Path::new("/not-exists"))
        .await
        .unwrap();
    assert!(notexist.is_none());
    let exist = h.service.entry_node(Path::new("/file.txt")).await.unwrap();
    match exist {
        None => unreachable!(),
        Some(exist) => {
            assert!(exist.entry().is_sync());
        }
    }
}

#[tokio::test]
#[should_panic(expected = "No such entry: /not-a-file.txt")]
async fn operate_fail_missing() {
    let h = harness(Dataset::empty()).await;
    h.service
        .clone()
        .operate(Operation::Sync("/not-a-file.txt".into()))
        .await
        .unwrap_display();
}

#[tokio::test]
#[should_panic(expected = "Expected an absolute path: file.txt")]
async fn operate_fail_relative() {
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file("/file.txt", "Test content")],
            remote: vec![],
        })
        .await
    };
    h.service
        .clone()
        .operate(Operation::Sync(PathBuf::from("file.txt")))
        .await
        .unwrap_display();
}

#[tokio::test]
async fn sync_remote_file() {
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![],
            remote: vec![Entry::txt_file("/file.txt", "Test content")],
        })
        .await
    };

    let path = "/file.txt";
    h.operate(Operation::Sync(path.into())).await;
    let content = h.local_file_content(path).await.expect("File should exist");
    assert_eq!(&content, "Test content");
}

#[tokio::test]
async fn sync_local_file() {
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file("/file.txt", "Test content")],
            remote: vec![],
        })
        .await
    };

    let path = "/file.txt";
    h.operate(Operation::Sync(path.into())).await;
    let content = h
        .remote_file_content(path)
        .await
        .expect("File should exist");
    assert_eq!(&content, "Test content");
}

#[tokio::test]
async fn sync_local_file_in_subdir() {
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::file_with_path_content("/only-local/deep/file.txt")],
            remote: vec![],
        })
        .await
    };
    let path = Path::new("/only-local/deep/file.txt");
    h.operate(Operation::Sync(path.to_path_buf())).await;

    assert!(h.has_sync_dir(path.parent().unwrap()).await);
    assert!(h.has_sync_file_with_path_content(path).await);

    let content = h
        .remote_file_content(path)
        .await
        .expect("File should exist");
    assert_eq!(&content, path.as_str());
}

#[tokio::test]
async fn sync_remote_deep_file_creates_local_dirs() {
    let path = "/dir/dir/file.txt";

    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![],
            remote: vec![Entry::txt_file(path, "Test content")],
        })
        .await
    };

    h.operate(Operation::Sync(path.into())).await;

    assert!(h.has_local_dir("/dir/dir").await);
    assert!(h.has_local_file("/dir/dir/file.txt").await);
}

#[tokio::test]
async fn sync_remote_dir_without_files() {
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![],
            remote: vec![
                Entry::file_with_path_content("/file.txt"),
                Entry::file_with_path_content("/dir/file.txt"),
                Entry::file_with_path_content("/dir/dir/file.txt"),
            ],
        })
        .await
    };

    h.operate(Operation::Sync("/dir/dir".into())).await;

    assert!(h.has_local_dir("/dir").await);
    assert!(h.has_local_dir("/dir/dir").await);
    assert!(!h.has_local_file("/file.txt").await);
    assert!(!h.has_local_file("/dir/file.txt").await);
    assert!(!h.has_local_file("/dir/dir/file.txt").await);
}

#[tokio::test]
async fn sync_remote_dir_deep_and_stats() {
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![],
            remote: vec![
                Entry::file_with_path_content("/file.txt"),
                Entry::file_with_path_content("/dir/file1.txt"),
                Entry::file_with_path_content("/dir/file2.txt"),
                Entry::file_with_path_content("/dir/dir/file1.txt"),
                Entry::file_with_path_content("/dir/dir/file2.txt"),
            ],
        })
        .await
    };

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        stat::Tree {
            local: stat::Dir {
                data: 0,
                dirs: 1, // root
                files: 0,
            },
            remote: stat::Dir {
                data: 9 + 2 * 14 + 2 * 18,
                dirs: 3,
                files: 5,
            },
            node: stat::Node {
                nodes: 8,
                sync: 1, // root
                conflicts: 0,
            },
        },
    );

    // will sync all except "/file.txt"

    h.operate(Operation::SyncDeep(PathBuf::from("/dir"))).await;

    assert!(!h.has_local_file("/file.txt").await);
    assert!(h.has_sync_dir("/dir").await);
    assert!(h.has_sync_dir("/dir/dir").await);
    assert!(h.has_sync_file_with_path_content("/dir/file1.txt").await);
    assert!(h.has_sync_file_with_path_content("/dir/file2.txt").await);
    assert!(
        h.has_sync_file_with_path_content("/dir/dir/file1.txt")
            .await
    );
    assert!(
        h.has_sync_file_with_path_content("/dir/dir/file2.txt")
            .await
    );

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        stat::Tree {
            local: stat::Dir {
                data: 2 * 14 + 2 * 18,
                dirs: 3,
                files: 4,
            },
            remote: stat::Dir {
                data: 9 + 2 * 14 + 2 * 18,
                dirs: 3,
                files: 5,
            },
            node: stat::Node {
                nodes: 8,
                sync: 7,
                conflicts: 0,
            },
        },
    );
}

#[tokio::test]
async fn detects_conflict() {
    let path = Path::new("/conflict.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Newer test content").with_age(0)],
            remote: vec![Entry::txt_file(path, "Older test content").with_age(10)],
        })
        .await
    };

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        stat::Tree {
            local: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            remote: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            node: stat::Node {
                nodes: 2,
                sync: 2, // sync include the conflicts
                conflicts: 1,
            },
        }
    );

    let conflict = h.entry_node(path).await.unwrap();
    assert!(matches!(
        conflict.entry(),
        Entry::Sync {
            conflict: Some(Conflict::LocalNewer),
            ..
        }
    ));
}

#[tokio::test]
async fn resolve_keep_newer_local() {
    let path = Path::new("/conflict.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Newer test content").with_age(0)],
            remote: vec![Entry::txt_file(path, "Older test content").with_age(10)],
        })
        .await
    };

    h.operate(Operation::Resolve(
        path.to_path_buf(),
        ResolutionMethod::ReplaceOlderByNewer,
    ))
    .await;

    assert_eq!(
        h.local_file_content(path).await.expect("File should exist"),
        "Newer test content"
    );
    assert_eq!(
        h.remote_file_content(path)
            .await
            .expect("File should exist"),
        "Newer test content"
    );

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        stat::Tree {
            local: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            remote: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            node: stat::Node {
                nodes: 2,
                sync: 2,
                conflicts: 0,
            },
        }
    );
}

#[tokio::test]
async fn resolve_keep_newer_remote() {
    let path = Path::new("/conflict.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Older test content").with_age(10)],
            remote: vec![Entry::txt_file(path, "Newer test content").with_age(0)],
        })
        .await
    };
    h.operate(Operation::Resolve(
        path.to_path_buf(),
        ResolutionMethod::ReplaceOlderByNewer,
    ))
    .await;

    assert_eq!(
        h.local_file_content(path).await.expect("File should exist"),
        "Newer test content"
    );
    assert_eq!(
        h.remote_file_content(path)
            .await
            .expect("File should exist"),
        "Newer test content"
    );
}

#[tokio::test]
async fn resolve_create_local_copy() {
    let path = Path::new("/conflict.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Older test content").with_age(10)],
            remote: vec![Entry::txt_file(path, "Newer test content").with_age(0)],
        })
        .await
    };

    h.operate(Operation::Resolve(
        path.to_path_buf(),
        ResolutionMethod::CreateLocalCopy,
    ))
    .await;

    assert_eq!(
        h.local_file_content(path).await.expect("File should exist"),
        "Newer test content"
    );
    assert_eq!(
        h.local_file_content("/conflict-copy.txt")
            .await
            .expect("File should exist"),
        "Older test content"
    );
    assert_eq!(
        h.remote_file_content(path)
            .await
            .expect("File should exist"),
        "Newer test content"
    );

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        // root dir itself is included in the stats
        stat::Tree {
            local: stat::Dir {
                data: 2 * 18,
                dirs: 1,
                files: 2,
            },
            remote: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            node: stat::Node {
                nodes: 3,
                sync: 2,
                conflicts: 0,
            },
        },
    );
}

#[tokio::test]
async fn resolve_delete_local() {
    let path = Path::new("/conflict.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Older test content").with_age(10)],
            remote: vec![Entry::txt_file(path, "Newer test content").with_age(0)],
        })
        .await
    };

    h.operate(Operation::Resolve(
        path.to_path_buf(),
        ResolutionMethod::DeleteLocal,
    ))
    .await;

    assert!(
        h.has_remote_file_with_content(path, "Newer test content")
            .await
    );
    assert!(!h.has_local_file(path).await);

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        // root dir itself is included in the stats
        stat::Tree {
            local: stat::Dir {
                data: 0,
                dirs: 1,
                files: 0,
            },
            remote: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            node: stat::Node {
                nodes: 2,
                sync: 1,
                conflicts: 0,
            },
        },
    );
}

#[tokio::test]
async fn resolve_delete_remote() {
    let path = Path::new("/conflict.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Older test content").with_age(10)],
            remote: vec![Entry::txt_file(path, "Newer test content").with_age(0)],
        })
        .await
    };

    h.operate(Operation::Resolve(
        path.to_path_buf(),
        ResolutionMethod::DeleteRemote,
    ))
    .await;

    assert_eq!(
        h.local_file_content(path).await.expect("File should exist"),
        "Older test content"
    );
    assert!(!h.has_remote_file(path).await);

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        // root dir itself is included in the stats
        stat::Tree {
            local: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            remote: stat::Dir {
                data: 0,
                dirs: 1,
                files: 0,
            },
            node: stat::Node {
                nodes: 2,
                sync: 1,
                conflicts: 0,
            },
        },
    );
}

#[tokio::test]
async fn resolve_delete_older() {
    let path = Path::new("/conflict.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Older test content").with_age(10)],
            remote: vec![Entry::txt_file(path, "Newer test content").with_age(0)],
        })
        .await
    };

    h.operate(Operation::Resolve(
        path.to_path_buf(),
        ResolutionMethod::DeleteOlder,
    ))
    .await;

    assert_eq!(
        h.remote_file_content(path)
            .await
            .expect("File should exist"),
        "Newer test content"
    );
    assert!(!h.has_local_file(path).await);

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        // root dir itself is included in the stats
        stat::Tree {
            local: stat::Dir {
                data: 0,
                dirs: 1,
                files: 0,
            },
            remote: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            node: stat::Node {
                nodes: 2,
                sync: 1,
                conflicts: 0,
            },
        },
    );
}

#[tokio::test]
async fn resolve_delete_newer() {
    let path = Path::new("/conflict.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Older test content").with_age(10)],
            remote: vec![Entry::txt_file(path, "Newer test content").with_age(0)],
        })
        .await
    };

    h.operate(Operation::Resolve(
        path.to_path_buf(),
        ResolutionMethod::DeleteNewer,
    ))
    .await;

    assert_eq!(
        h.local_file_content(path).await.expect("File should exist"),
        "Older test content"
    );
    assert!(!h.has_remote_file(path).await);

    assert_eq!(
        h.tree_stats(Path::root()).await.unwrap(),
        // root dir itself is included in the stats
        stat::Tree {
            local: stat::Dir {
                data: 18,
                dirs: 1,
                files: 1,
            },
            remote: stat::Dir {
                data: 0,
                dirs: 1,
                files: 0,
            },
            node: stat::Node {
                nodes: 2,
                sync: 1,
                conflicts: 0,
            },
        },
    );
}

#[tokio::test]
async fn delete_local() {
    let path = Path::new("/file.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Test content").with_age(0)],
            remote: vec![Entry::txt_file(path, "Test content").with_age(0)],
        })
        .await
    };
    h.operate(Operation::Delete(path.to_path_buf(), DeletionMethod::Local))
        .await;
    assert!(!h.has_local_file(path).await);
    assert!(h.has_remote_file(path).await);
}

#[tokio::test]
async fn delete_remote() {
    let path = Path::new("/file.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Test content").with_age(0)],
            remote: vec![Entry::txt_file(path, "Test content").with_age(0)],
        })
        .await
    };
    h.operate(Operation::Delete(
        path.to_path_buf(),
        DeletionMethod::Remote,
    ))
    .await;
    assert!(h.has_local_file(path).await);
    assert!(!h.has_remote_file(path).await);
}

#[tokio::test]
async fn delete_both() {
    let path = Path::new("/file.txt");
    let h = {
        use dataset::Entry;
        harness(Dataset {
            local: vec![Entry::txt_file(path, "Test content").with_age(0)],
            remote: vec![Entry::txt_file(path, "Test content").with_age(0)],
        })
        .await
    };
    h.operate(Operation::Delete(path.to_path_buf(), DeletionMethod::All))
        .await;
    assert!(!h.has_local_file(path).await);
    assert!(!h.has_remote_file(path).await);
}
