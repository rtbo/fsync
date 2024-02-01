use std::cmp::Ordering;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Metadata {
    mtime: DateTime<Utc>,
    size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Conflict {
    LocalNewer {
        path: PathBuf,
        local: Metadata,
        remote: Metadata,
    },
    LocalOlder {
        path: PathBuf,
        local: Metadata,
        remote: Metadata,
    },
    LocalBigger {
        path: PathBuf,
        mtime: DateTime<Utc>,
        local: u64,
        remote: u64,
    },
    LocalSmaller {
        path: PathBuf,
        mtime: DateTime<Utc>,
        local: u64,
        remote: u64,
    },
    LocalFileRemoteDir {
        path: PathBuf,
        local: Metadata,
    },
    LocalDirRemoteFile {
        path: PathBuf,
        remote: Metadata,
    },
}

impl Conflict {
    pub fn check(local: &crate::Metadata, remote: &crate::Metadata) -> Option<Self> {
        use crate::Metadata::{Directory, Regular, Special, Symlink};
        debug_assert_eq!(local.path(), remote.path());

        match (local, remote) {
            (Special { .. }, _) | (_, Special { .. }) => unimplemented!("special file conflicts"),
            (Symlink { .. }, _) | (_, Symlink { .. }) => unimplemented!("symlink file conflicts"),
            (Directory { .. }, Directory { .. }) => None,
            (Regular { path, size, mtime }, Directory { .. }) => {
                Some(Conflict::LocalFileRemoteDir {
                    path: path.clone(),
                    local: Metadata {
                        mtime: *mtime,
                        size: *size,
                    },
                })
            }
            (Directory { .. }, Regular { path, size, mtime }) => {
                Some(Conflict::LocalDirRemoteFile {
                    path: path.clone(),
                    remote: Metadata {
                        mtime: *mtime,
                        size: *size,
                    },
                })
            }
            (
                Regular {
                    path,
                    size: loc_sz,
                    mtime: loc_mt,
                },
                Regular {
                    size: rem_sz,
                    mtime: rem_mt,
                    ..
                },
            ) => {
                let loc_sz = *loc_sz;
                let rem_sz = *rem_sz;
                let loc_mt = *loc_mt;
                let rem_mt = *rem_mt;
                match crate::compare_mtime(loc_mt, rem_mt) {
                    Ordering::Less => Some(Conflict::LocalOlder {
                        path: path.clone(),
                        local: Metadata {
                            mtime: loc_mt,
                            size: loc_sz,
                        },
                        remote: Metadata {
                            mtime: rem_mt,
                            size: rem_sz,
                        },
                    }),
                    Ordering::Greater => Some(Conflict::LocalNewer {
                        path: path.clone(),
                        local: Metadata {
                            mtime: loc_mt,
                            size: loc_sz,
                        },
                        remote: Metadata {
                            mtime: rem_mt,
                            size: rem_sz,
                        },
                    }),
                    Ordering::Equal if loc_sz < rem_sz => {
                        let mtime = loc_mt + (rem_mt - loc_mt) / 2; // compensation of tolerance of compare_mtime
                        Some(Conflict::LocalSmaller {
                            path: path.clone(),
                            mtime,
                            local: loc_sz,
                            remote: rem_sz,
                        })
                    }
                    Ordering::Equal if loc_sz > rem_sz => {
                        let mtime = loc_mt + (rem_mt - loc_mt) / 2; // compensation of tolerance of compare_mtime
                        Some(Conflict::LocalBigger {
                            path: path.clone(),
                            mtime,
                            local: loc_sz,
                            remote: rem_sz,
                        })
                    }
                    Ordering::Equal => None,
                }
            }
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::LocalBigger { path, .. } => path.as_path(),
            Self::LocalSmaller { path, .. } => path.as_path(),
            Self::LocalNewer { path, .. } => path.as_path(),
            Self::LocalOlder { path, .. } => path.as_path(),
            Self::LocalFileRemoteDir { path, .. } => path.as_path(),
            Self::LocalDirRemoteFile { path, .. } => path.as_path(),
        }
    }
}
