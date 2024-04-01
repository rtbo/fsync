use std::{cmp::Ordering, fmt};

use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TypeDef)]
#[serde(rename_all = "camelCase")]
pub enum Conflict {
    LocalNewer,
    LocalOlder,
    LocalBigger,
    LocalSmaller,
    LocalFileRemoteDir,
    LocalDirRemoteFile,
}

impl Conflict {
    pub fn check(local: &crate::Metadata, remote: &crate::Metadata) -> Option<Self> {
        use crate::Metadata::{Directory, Regular};
        debug_assert_eq!(local.path(), remote.path());

        match (local, remote) {
            (Directory { .. }, Directory { .. }) => None,
            (Regular { .. }, Directory { .. }) => Some(Self::LocalFileRemoteDir),
            (Directory { .. }, Regular { .. }) => Some(Self::LocalDirRemoteFile),
            (
                Regular {
                    size: loc_sz,
                    mtime: loc_mt,
                    ..
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
                    Ordering::Less => Some(Self::LocalOlder),
                    Ordering::Greater => Some(Self::LocalNewer),
                    Ordering::Equal if loc_sz < rem_sz => Some(Conflict::LocalSmaller),
                    Ordering::Equal if loc_sz > rem_sz => Some(Self::LocalBigger),
                    Ordering::Equal => None,
                }
            }
        }
    }
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalBigger => f.write_str("local is bigger (but modified at same time)"),
            Self::LocalSmaller => f.write_str("local is smaller (but modified at same time)"),
            Self::LocalNewer => f.write_str("local is newer"),
            Self::LocalOlder => f.write_str("local is older"),
            Self::LocalFileRemoteDir => f.write_str("local is file, remote is dir"),
            Self::LocalDirRemoteFile => f.write_str("local is dir, remote is file"),
        }
    }
}
