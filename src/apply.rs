use crate::delta::Operation;
use anyhow::{anyhow, Result};
use filetime::{set_file_times, FileTime};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Apply a patch to a root directory.
/// If `dry_run` is true, validate and print operations but do not modify disk.
/// This is a best-effort transactional apply: on failure, attempt to rollback applied ops.
pub fn apply_patch(root: &str, patch: &[Operation], dry_run: bool) -> Result<()> {
    if dry_run {
        println!("Dry-run: {} operations", patch.len());
        for op in patch {
            println!("{}", serde_json::to_string(op)?);
        }
        return Ok(());
    }

    let root = Path::new(root);
    if !root.exists() {
        return Err(anyhow!("root path does not exist: {}", root.display()));
    }

    // Create a temporary backup directory for files we will overwrite/delete/move
    let backup_dir = TempDir::new()?;
    let mut applied_ops: Vec<Operation> = Vec::new();

    // Helper to backup a path if it exists
    let mut backup_path = |p: &Path| -> Result<Option<PathBuf>> {
        if p.exists() {
            let rel = p.strip_prefix(root).unwrap_or(p);
            let dest = backup_dir.path().join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if p.is_file() || p.is_symlink() {
                fs::copy(p, &dest)?;
            } else if p.is_dir() {
                // copy directory metadata by creating dir
                fs::create_dir_all(&dest)?;
            }
            Ok(Some(dest))
        } else {
            Ok(None)
        }
    };

    // Apply operations in order, recording applied ops for rollback
    for op in patch {
        let res = match op {
            Operation::CreateDir { path } => {
                let p = root.join(path.strip_prefix("/").unwrap_or(path));
                if !p.exists() {
                    fs::create_dir_all(&p)?;
                }
                Ok(())
            }
            Operation::DeleteDir { path } => {
                let p = root.join(path.strip_prefix("/").unwrap_or(path));
                if p.exists() {
                    // backup
                    let _ = backup_path(&p)?;
                    fs::remove_dir_all(&p)?;
                }
                Ok(())
            }
            Operation::CreateFile { path, content, mode, mtime, .. } => {
                let p = root.join(path.strip_prefix("/").unwrap_or(path));
                if let Some(parent) = p.parent() {
                    fs::create_dir_all(parent)?;
                }
                // backup if exists
                let _ = backup_path(&p)?;
                let mut f = fs::File::create(&p)?;
                if let Some(c) = content {
                    f.write_all(c.as_bytes())?;
                }
                if let Some(m) = mode {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&p, fs::Permissions::from_mode(*m))?;
                    }
                }
                if let Some(mt) = mtime {
                    let ft = FileTime::from_unix_time(*mt as i64, 0);
                    set_file_times(&p, ft, ft)?;
                }
                Ok(())
            }
            Operation::DeleteFile { path } => {
                let p = root.join(path.strip_prefix("/").unwrap_or(path));
                if p.exists() {
                    let _ = backup_path(&p)?;
                    if p.is_file() || p.is_symlink() {
                        fs::remove_file(&p)?;
                    } else if p.is_dir() {
                        fs::remove_dir_all(&p)?;
                    }
                }
                Ok(())
            }
            Operation::ModifyFile { path, content, .. } => {
                let p = root.join(path.strip_prefix("/").unwrap_or(path));
                if p.exists() {
                    let _ = backup_path(&p)?;
                } else {
                    if let Some(parent) = p.parent() {
                        fs::create_dir_all(parent)?;
                    }
                }
                let mut f = fs::File::create(&p)?;
                if let Some(c) = content {
                    f.write_all(c.as_bytes())?;
                }
                Ok(())
            }
            Operation::Move { from, to } => {
                let src = root.join(from.strip_prefix("/").unwrap_or(from));
                let dst = root.join(to.strip_prefix("/").unwrap_or(to));
                if src.exists() {
                    if let Some(parent) = dst.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    // backup destination if exists
                    let _ = backup_path(&dst)?;
                    fs::rename(&src, &dst)?;
                } else {
                    return Err(anyhow!("move source does not exist: {}", src.display()));
                }
                Ok(())
            }
            Operation::Chmod { path, mode } => {
                let p = root.join(path.strip_prefix("/").unwrap_or(path));
                if p.exists() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&p, fs::Permissions::from_mode(*mode))?;
                    }
                }
                Ok(())
            }
            Operation::Utimes { path, mtime, atime } => {
                let p = root.join(path.strip_prefix("/").unwrap_or(path));
                if p.exists() {
                    let m = FileTime::from_unix_time(*mtime as i64, 0);
                    let a = atime.map(|t| FileTime::from_unix_time(t as i64, 0)).unwrap_or(m);
                    set_file_times(&p, a, m)?;
                }
                Ok(())
            }
            Operation::Symlink { path, target } => {
                let p = root.join(path.strip_prefix("/").unwrap_or(path));
                if p.exists() {
                    let _ = backup_path(&p)?;
                    if p.is_file() || p.is_symlink() {
                        fs::remove_file(&p)?;
                    } else if p.is_dir() {
                        fs::remove_dir_all(&p)?;
                    }
                }
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(target, &p)?;
                }
                #[cfg(windows)]
                {
                    if target.ends_with('/') || target.ends_with('\\') {
                        std::os::windows::fs::symlink_dir(target, &p)?;
                    } else {
                        std::os::windows::fs::symlink_file(target, &p)?;
                    }
                }
                Ok(())
            }
        };

        if let Err(e) = res {
            // Attempt rollback
            eprintln!("Error applying op: {:?} -> {}. Attempting rollback: {}", op, serde_json::to_string(op).unwrap_or_default(), e);
            if let Err(rb_err) = rollback_from_backup(root, backup_dir.path()) {
                eprintln!("Rollback failed: {}", rb_err);
            }
            return Err(e);
        } else {
            applied_ops.push(op.clone());
        }
    }

    // If we reach here, all ops applied successfully.
    // Backup dir will be dropped (TempDir) — keep it for debugging if desired.
    Ok(())
}

/// Rollback by copying files from backup_dir back into root
fn rollback_from_backup(root: &Path, backup_dir: &Path) -> Result<()> {
    if !backup_dir.exists() {
        return Ok(());
    }
    // Walk backup_dir and copy entries back
    for entry in walkdir::WalkDir::new(backup_dir) {
        let entry = entry?;
        let path = entry.path();
        if path == backup_dir {
            continue;
        }
        let rel = path.strip_prefix(backup_dir)?;
        let dest = root.join(rel);
        if entry.file_type().is_dir() {
            if !dest.exists() {
                fs::create_dir_all(&dest)?;
            }
        } else if entry.file_type().is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &dest)?;
        } else if entry.file_type().is_symlink() {
            // best-effort: remove existing and recreate symlink if possible
            if dest.exists() {
                let _ = fs::remove_file(&dest);
            }
            #[cfg(unix)]
            {
                let target = fs::read_link(path)?;
                std::os::unix::fs::symlink(target, &dest)?;
            }
        }
    }
    Ok(())
}
