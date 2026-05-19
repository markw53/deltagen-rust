use crate::snapshot::{Entry, EntryType, Snapshot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Operation kinds
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Operation {
    #[serde(rename = "create_file")]
    CreateFile {
        path: String,
        content_hash: Option<String>,
        mode: Option<u32>,
        mtime: Option<u64>,
        content: Option<String>,
    },
    #[serde(rename = "delete_file")]
    DeleteFile { path: String },
    #[serde(rename = "modify_file")]
    ModifyFile {
        path: String,
        old_hash: Option<String>,
        new_hash: Option<String>,
        content: Option<String>,
    },
    #[serde(rename = "create_dir")]
    CreateDir { path: String },
    #[serde(rename = "delete_dir")]
    DeleteDir { path: String },
    #[serde(rename = "move")]
    Move { from: String, to: String },
    #[serde(rename = "chmod")]
    Chmod { path: String, mode: u32 },
    #[serde(rename = "utimes")]
    Utimes { path: String, mtime: u64, atime: Option<u64> },
    #[serde(rename = "symlink")]
    Symlink { path: String, target: String },
}

/// Compute a deterministic patch from src -> dst
pub fn compute_delta(src: &Snapshot, dst: &Snapshot) -> anyhow::Result<Vec<Operation>> {
    // Build path maps
    let src_map = src.path_map();
    let dst_map = dst.path_map();

    // Collect sets
    let mut ops: Vec<Operation> = Vec::new();

    // 1) Handle identical paths and modifications
    let mut src_only_paths: Vec<String> = Vec::new();
    let mut dst_only_paths: Vec<String> = Vec::new();

    for p in src_map.keys() {
        if !dst_map.contains_key(p) {
            src_only_paths.push(p.clone());
        }
    }
    for p in dst_map.keys() {
        if !src_map.contains_key(p) {
            dst_only_paths.push(p.clone());
        }
    }

    // For paths present in both, check for modifications or metadata changes
    let mut metadata_ops: Vec<Operation> = Vec::new();
    for (p, s_entry) in &src_map {
        if let Some(d_entry) = dst_map.get(p) {
            if s_entry.entry_type != d_entry.entry_type {
                // Type changed: delete old, create new
                match s_entry.entry_type {
                    EntryType::File => ops.push(Operation::DeleteFile { path: p.clone() }),
                    EntryType::Dir => ops.push(Operation::DeleteDir { path: p.clone() }),
                    EntryType::Symlink => ops.push(Operation::DeleteFile { path: p.clone() }),
                }
                match d_entry.entry_type {
                    EntryType::File => ops.push(Operation::CreateFile {
                        path: p.clone(),
                        content_hash: d_entry.hash.clone(),
                        mode: d_entry.mode,
                        mtime: d_entry.mtime,
                        content: None,
                    }),
                    EntryType::Dir => ops.push(Operation::CreateDir { path: p.clone() }),
                    EntryType::Symlink => ops.push(Operation::Symlink {
                        path: p.clone(),
                        target: d_entry.link_target.clone().unwrap_or_default(),
                    }),
                }
                continue;
            }

            match (&s_entry.entry_type, &d_entry.entry_type) {
                (EntryType::File, EntryType::File) => {
                    if s_entry.hash != d_entry.hash {
                        metadata_ops.push(Operation::ModifyFile {
                            path: p.clone(),
                            old_hash: s_entry.hash.clone(),
                            new_hash: d_entry.hash.clone(),
                            content: None,
                        });
                    } else {
                        // same content, maybe metadata changes
                        if s_entry.mode != d_entry.mode {
                            if let Some(m) = d_entry.mode {
                                metadata_ops.push(Operation::Chmod { path: p.clone(), mode: m });
                            }
                        }
                        if s_entry.mtime != d_entry.mtime {
                            if let Some(mtime) = d_entry.mtime {
                                metadata_ops.push(Operation::Utimes { path: p.clone(), mtime, atime: None });
                            }
                        }
                    }
                }
                (EntryType::Dir, EntryType::Dir) => {
                    // directories: maybe metadata changes (mode/mtime)
                    if s_entry.mode != d_entry.mode {
                        if let Some(m) = d_entry.mode {
                            metadata_ops.push(Operation::Chmod { path: p.clone(), mode: m });
                        }
                    }
                }
                (EntryType::Symlink, EntryType::Symlink) => {
                    if s_entry.link_target != d_entry.link_target {
                        metadata_ops.push(Operation::Symlink {
                            path: p.clone(),
                            target: d_entry.link_target.clone().unwrap_or_default(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // 2) Detect moves for files by hash
    // Build hash -> paths for src-only and dst-only files
    let mut src_hash_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut dst_hash_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut src_file_sizes: HashMap<String, u64> = HashMap::new();

    for p in &src_only_paths {
        if let Some(e) = src_map.get(p) {
            if let EntryType::File = e.entry_type {
                if let Some(h) = &e.hash {
                    src_hash_map.entry(h.clone()).or_default().push(p.clone());
                    if let Some(sz) = e.size {
                        src_file_sizes.insert(p.clone(), sz);
                    }
                }
            }
        }
    }
    for p in &dst_only_paths {
        if let Some(e) = dst_map.get(p) {
            if let EntryType::File = e.entry_type {
                if let Some(h) = &e.hash {
                    dst_hash_map.entry(h.clone()).or_default().push(p.clone());
                }
            }
        }
    }

    // Candidate moves: for each hash present in both maps, pair up paths.
    // We'll greedily match by preferring larger source file sizes first (minimize bytes moved).
    let mut move_pairs: Vec<(String, String, u64)> = Vec::new(); // (from, to, size)
    for (hash, dst_paths) in &dst_hash_map {
        if let Some(src_paths) = src_hash_map.get(hash) {
            // produce all candidate pairs
            for dst_p in dst_paths {
                for src_p in src_paths {
                    let size = *src_file_sizes.get(src_p).unwrap_or(&0u64);
                    move_pairs.push((src_p.clone(), dst_p.clone(), size));
                }
            }
        }
    }

    // Sort candidate pairs: prefer larger size, then lexicographically smaller source path
    move_pairs.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });

    // Greedy selection ensuring non-overlapping paths
    let mut used_src: HashSet<String> = HashSet::new();
    let mut used_dst: HashSet<String> = HashSet::new();
    let mut selected_moves: Vec<(String, String)> = Vec::new();

    for (src_p, dst_p, _) in move_pairs {
        if used_src.contains(&src_p) || used_dst.contains(&dst_p) {
            continue;
        }
        used_src.insert(src_p.clone());
        used_dst.insert(dst_p.clone());
        selected_moves.push((src_p.clone(), dst_p.clone()));
    }

    // Emit moves (deterministic order: sort by from path)
    selected_moves.sort_by(|a, b| a.0.cmp(&b.0));
    for (from, to) in &selected_moves {
        ops.push(Operation::Move { from: from.clone(), to: to.clone() });
    }

    // Remove moved paths from src_only_paths and dst_only_paths
    let moved_src: HashSet<String> = selected_moves.iter().map(|(s, _)| s.clone()).collect();
    let moved_dst: HashSet<String> = selected_moves.iter().map(|(_, d)| d.clone()).collect();

    src_only_paths.retain(|p| !moved_src.contains(p));
    dst_only_paths.retain(|p| !moved_dst.contains(p));

    // 3) Remaining deletes (delete children before parents)
    // Sort src_only_paths by depth descending, then lexicographically
    src_only_paths.sort_by(|a, b| {
        let da = a.matches('/').count();
        let db = b.matches('/').count();
        db.cmp(&da).then_with(|| a.cmp(b))
    });

    for p in &src_only_paths {
        if let Some(e) = src_map.get(p) {
            match e.entry_type {
                EntryType::File => ops.push(Operation::DeleteFile { path: p.clone() }),
                EntryType::Dir => ops.push(Operation::DeleteDir { path: p.clone() }),
                EntryType::Symlink => ops.push(Operation::DeleteFile { path: p.clone() }),
            }
        }
    }

    // 4) Remaining creates (parents before children)
    // Sort dst_only_paths by depth ascending
    dst_only_paths.sort_by(|a, b| {
        let da = a.matches('/').count();
        let db = b.matches('/').count();
        da.cmp(&db).then_with(|| a.cmp(b))
    });

    for p in &dst_only_paths {
        if let Some(e) = dst_map.get(p) {
            match e.entry_type {
                EntryType::Dir => ops.push(Operation::CreateDir { path: p.clone() }),
                EntryType::File => ops.push(Operation::CreateFile {
                    path: p.clone(),
                    content_hash: e.hash.clone(),
                    mode: e.mode,
                    mtime: e.mtime,
                    content: None,
                }),
                EntryType::Symlink => ops.push(Operation::Symlink {
                    path: p.clone(),
                    target: e.link_target.clone().unwrap_or_default(),
                }),
            }
        }
    }

    // 5) Finally, append metadata ops deterministically sorted
    metadata_ops.sort_by(|a, b| {
        let ka = op_sort_key(a);
        let kb = op_sort_key(b);
        ka.cmp(&kb)
    });
    ops.extend(metadata_ops);

    Ok(ops)
}

/// Deterministic key for sorting metadata ops
fn op_sort_key(op: &Operation) -> (u8, String) {
    match op {
        Operation::Chmod { path, .. } => (5, path.clone()),
        Operation::Utimes { path, .. } => (6, path.clone()),
        Operation::ModifyFile { path, .. } => (4, path.clone()),
        Operation::Symlink { path, .. } => (3, path.clone()),
        _ => (9, "".to_string()),
    }
}

/// Invert a patch (produce operations that undo the given patch)
pub fn invert_patch(patch: &[Operation]) -> anyhow::Result<Vec<Operation>> {
    // This inversion is conservative: it inverts structural ops in reverse order.
    // For create_file we emit delete_file; for delete_file we emit create_file with no content (best-effort).
    let mut inv: Vec<Operation> = Vec::new();
    for op in patch.iter().rev() {
        match op {
            Operation::CreateFile { path, content_hash, mode, mtime, content } => {
                inv.push(Operation::DeleteFile { path: path.clone() });
                // Note: we cannot reconstruct original content here.
            }
            Operation::DeleteFile { path } => {
                inv.push(Operation::CreateFile {
                    path: path.clone(),
                    content_hash: None,
                    mode: None,
                    mtime: None,
                    content: None,
                });
            }
            Operation::Move { from, to } => {
                inv.push(Operation::Move { from: to.clone(), to: from.clone() });
            }
            Operation::ModifyFile { path, old_hash, new_hash, content: _ } => {
                inv.push(Operation::ModifyFile {
                    path: path.clone(),
                    old_hash: new_hash.clone(),
                    new_hash: old_hash.clone(),
                    content: None,
                });
            }
            Operation::CreateDir { path } => inv.push(Operation::DeleteDir { path: path.clone() }),
            Operation::DeleteDir { path } => inv.push(Operation::CreateDir { path: path.clone() }),
            Operation::Chmod { path, mode } => inv.push(Operation::Chmod { path: path.clone(), mode: *mode }),
            Operation::Utimes { path, mtime, atime } => inv.push(Operation::Utimes { path: path.clone(), mtime: *mtime, atime: *atime }),
            Operation::Symlink { path, target: _ } => inv.push(Operation::DeleteFile { path: path.clone() }),
        }
    }
    Ok(inv)
}
