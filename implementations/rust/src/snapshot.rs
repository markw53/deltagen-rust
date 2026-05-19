use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Type of entry in a snapshot
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    File,
    Dir,
    Symlink,
}

/// Single entry record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,           // POSIX-style relative path
    pub entry_type: EntryType,  // file|dir|symlink
    pub size: Option<u64>,
    pub mode: Option<u32>,
    pub mtime: Option<u64>,     // unix seconds
    pub hash: Option<String>,   // sha256 hex for files
    pub link_target: Option<String>,
}

/// Snapshot of a tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub root: String,
    pub entries: Vec<Entry>,
}

impl Snapshot {
    /// Scan a directory on disk and produce a Snapshot.
    pub fn scan<P: AsRef<std::path::Path>>(root: P) -> anyhow::Result<Self> {
        crate::util::scan_dir(root.as_ref())
    }

    /// Helper: build a map path -> entry
    pub fn path_map(&self) -> std::collections::HashMap<String, Entry> {
        let mut m = std::collections::HashMap::new();
        for e in &self.entries {
            m.insert(e.path.clone(), e.clone());
        }
        m
    }
}
