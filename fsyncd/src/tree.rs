
use camino::Utf8PathBuf;
use fsync::storage::{EntryType, Entry};

pub enum TreeEntry {
    Local {
        path: Utf8PathBuf,
        typ: EntryType,
    },
    Remote {
        path: Utf8PathBuf,
        id: String,
        typ: EntryType,
    },
    Both {
        path: Utf8PathBuf, 
        local_typ: EntryType,
        remote_id: String,
        remote_typ: EntryType,
    }
}