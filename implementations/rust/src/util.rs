use crate::snapshot::{Entry, EntryType, Snapshot};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Scan directory and produce a Snapshot.
/// This implementation records basic metadata and computes SHA256 for files.
pub fn scan_dir(root: &Path) -> Result<Snapshot> {
    let mut entries = Vec::new();
    let root_str = root.to_string_lossy().to_string();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let path = entry.path();
        if path == root {
            // skip root itself as an entry; entries are relative
            continue;
        }
        let rel = path.strip_prefix(root)?.to_string_lossy().to_string();
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_dir() {
            entries.push(Entry {
                path: rel,
                entry_type: EntryType::Dir,
                size: None,
                mode: Some(get_mode(&metadata)?),
                mtime: metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
                hash: None,
                link_target: None,
            });
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(path)?.to_string_lossy().to_string();
            entries.push(Entry {
                path: rel,
                entry_type: EntryType::Symlink,
                size: None,
                mode: None,
                mtime: None,
                hash: None,
                link_target: Some(target),
            });
        } else if metadata.is_file() {
            let size = metadata.len();
            let mtime = metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
            let hash = compute_sha256(path)?;
            entries.push(Entry {
                path: rel,
                entry_type: EntryType::File,
                size: Some(size),
                mode: Some(get_mode(&metadata)?),
                mtime,
                hash: Some(hash),
                link_target: None,
            });
        }
    }

    // Sort entries deterministically by path
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(Snapshot { root: root_str, entries })
}

/// Compute SHA256 hex of a file
pub fn compute_sha256(path: &Path) -> Result<String> {
    let f = fs::File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(unix)]
fn get_mode(metadata: &std::fs::Metadata) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn get_mode(_metadata: &std::fs::Metadata) -> Result<u32> {
    // On non-unix platforms, return a default
    Ok(0)
}
