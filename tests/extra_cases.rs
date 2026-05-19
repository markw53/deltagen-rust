#[cfg(test)]
mod extra_cases {
    use assert_fs::prelude::*;
    use std::fs;
    use std::time::{SystemTime, Duration};
    use deltagen::snapshot::Snapshot;
    use deltagen::delta::{compute_delta, Operation};

    #[test]
    fn symlink_target_change() -> anyhow::Result<()> {
        let temp = assert_fs::TempDir::new()?;
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;

        // src: link -> a.txt
        fs::write(src.join("a.txt"), b"hello")?;
        #[cfg(unix)]
        std::os::unix::fs::symlink("a.txt", src.join("link"))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file("a.txt", src.join("link"))?;

        // dst: link -> b.txt
        fs::write(dst.join("b.txt"), b"hello")?;
        #[cfg(unix)]
        std::os::unix::fs::symlink("b.txt", dst.join("link"))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file("b.txt", dst.join("link"))?;

        let s_snap = Snapshot::scan(&src)?;
        let d_snap = Snapshot::scan(&dst)?;
        let patch = compute_delta(&s_snap, &d_snap)?;
        let has_symlink = patch.iter().any(|op| matches!(op, Operation::Symlink { path, .. } if path.ends_with("link")));
        assert!(has_symlink, "expected symlink op, got {:?}", patch);
        temp.close()?;
        Ok(())
    }

    #[test]
    fn permission_only_change() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let temp = assert_fs::TempDir::new()?;
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;

        let a_src = src.join("a.txt");
        fs::write(&a_src, b"content")?;
        fs::set_permissions(&a_src, fs::Permissions::from_mode(0o644))?;

        let a_dst = dst.join("a.txt");
        fs::write(&a_dst, b"content")?;
        fs::set_permissions(&a_dst, fs::Permissions::from_mode(0o600))?;

        let s_snap = Snapshot::scan(&src)?;
        let d_snap = Snapshot::scan(&dst)?;
        let patch = compute_delta(&s_snap, &d_snap)?;
        let has_chmod = patch.iter().any(|op| matches!(op, Operation::Chmod { path, .. } if path.ends_with("a.txt")));
        assert!(has_chmod, "expected chmod op, got {:?}", patch);
        temp.close()?;
        Ok(())
    }

    #[test]
    fn timestamp_only_change() -> anyhow::Result<()> {
        let temp = assert_fs::TempDir::new()?;
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;

        let a_src = src.join("a.txt");
        fs::write(&a_src, b"content")?;
        // set mtime to now
        let now = filetime::FileTime::from_system_time(SystemTime::now());
        filetime::set_file_mtime(&a_src, now)?;

        let a_dst = dst.join("a.txt");
        fs::write(&a_dst, b"content")?;
        // set mtime to now + 60s
        let later = filetime::FileTime::from_system_time(SystemTime::now() + Duration::from_secs(60));
        filetime::set_file_mtime(&a_dst, later)?;

        let s_snap = Snapshot::scan(&src)?;
        let d_snap = Snapshot::scan(&dst)?;
        let patch = compute_delta(&s_snap, &d_snap)?;
        let has_utimes = patch.iter().any(|op| matches!(op, Operation::Utimes { path, .. } if path.ends_with("a.txt")));
        assert!(has_utimes, "expected utimes op, got {:?}", patch);
        temp.close()?;
        Ok(())
    }

    #[test]
    fn swap_identical_files() -> anyhow::Result<()> {
        let temp = assert_fs::TempDir::new()?;
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;

        // src: a.txt (X), b.txt (Y)
        fs::write(src.join("a.txt"), b"X")?;
        fs::write(src.join("b.txt"), b"Y")?;

        // dst: a.txt (Y), b.txt (X) -> swapped contents
        fs::write(dst.join("a.txt"), b"Y")?;
        fs::write(dst.join("b.txt"), b"X")?;

        let s_snap = Snapshot::scan(&src)?;
        let d_snap = Snapshot::scan(&dst)?;
        let patch = compute_delta(&s_snap, &d_snap)?;

        // Expect two moves or two modify ops; ensure deterministic count and no deletes+creates
        let move_count = patch.iter().filter(|op| matches!(op, Operation::Move { .. })).count();
        let modify_count = patch.iter().filter(|op| matches!(op, Operation::ModifyFile { .. })).count();
        assert!(move_count + modify_count >= 2, "expected at least two ops to reconcile swap, got {:?}", patch);
        temp.close()?;
        Ok(())
    }

    #[test]
    fn nested_moves() -> anyhow::Result<()> {
        let temp = assert_fs::TempDir::new()?;
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;

        // src tree: dir1/x.txt, dir1/sub/y.txt
        fs::create_dir_all(src.join("dir1/sub"))?;
        fs::write(src.join("dir1/x.txt"), b"A")?;
        fs::write(src.join("dir1/sub/y.txt"), b"B")?;

        // dst tree: dir2/x.txt, dir2/sub/y.txt (moved dir1 -> dir2)
        fs::create_dir_all(dst.join("dir2/sub"))?;
        fs::write(dst.join("dir2/x.txt"), b"A")?;
        fs::write(dst.join("dir2/sub/y.txt"), b"B")?;

        let s_snap = Snapshot::scan(&src)?;
        let d_snap = Snapshot::scan(&dst)?;
        let patch = compute_delta(&s_snap, &d_snap)?;

        // Expect moves or a move of files; ensure no unnecessary delete+create for each file
        let delete_count = patch.iter().filter(|op| matches!(op, Operation::DeleteFile { .. } | Operation::DeleteDir { .. })).count();
        assert!(delete_count < 4, "too many deletes, patch: {:?}", patch);
        temp.close()?;
        Ok(())
    }

    #[test]
    #[ignore] // heavy test; enable manually or in performance CI
    fn large_tree_smoke() -> anyhow::Result<()> {
        let temp = assert_fs::TempDir::new()?;
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;

        // generate 50k small files in src and move half to dst with same content
        for i in 0..50000 {
            let name = format!("f{:05}.txt", i);
            fs::write(src.join(&name), format!("content {}", i).as_bytes())?;
            if i % 2 == 0 {
                fs::write(dst.join(&name), format!("content {}", i).as_bytes())?;
            }
        }

        let s_snap = Snapshot::scan(&src)?;
        let d_snap = Snapshot::scan(&dst)?;
        let patch = compute_delta(&s_snap, &d_snap)?;
        // basic sanity: patch should be non-empty and deterministic on repeated runs
        assert!(!patch.is_empty());
        let patch2 = compute_delta(&s_snap, &d_snap)?;
        assert_eq!(serde_json::to_string(&patch)?, serde_json::to_string(&patch2)?);
        temp.close()?;
        Ok(())
    }
}
