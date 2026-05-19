#[cfg(test)]
mod tests {
    use assert_fs::prelude::*;
    use std::fs;
    use deltagen::snapshot::Snapshot;
    use deltagen::delta::{compute_delta, Operation};

    #[test]
    fn identical_trees_produce_empty_patch() -> anyhow::Result<()> {
        let temp = assert_fs::TempDir::new()?;
        temp.child("a.txt").write_str("hello")?;
        temp.child("dir").create_dir_all()?;
        let src = deltagen::snapshot::Snapshot::scan(temp.path())?;
        let dst = deltagen::snapshot::Snapshot::scan(temp.path())?;
        let patch = compute_delta(&src, &dst)?;
        assert!(patch.is_empty());
        temp.close()?;
        Ok(())
    }

    #[test]
    fn rename_file_detected_as_move() -> anyhow::Result<()> {
        let temp = assert_fs::TempDir::new()?;
        temp.child("a.txt").write_str("same content")?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");
        fs::create_dir_all(&src_dir)?;
        fs::create_dir_all(&dst_dir)?;
        fs::write(src_dir.join("a.txt"), b"same content")?;
        fs::write(dst_dir.join("b.txt"), b"same content")?;
        let src = deltagen::snapshot::Snapshot::scan(&src_dir)?;
        let dst = deltagen::snapshot::Snapshot::scan(&dst_dir)?;
        let patch = compute_delta(&src, &dst)?;
        // Expect a move from a.txt -> b.txt
        let has_move = patch.iter().any(|op| match op {
            Operation::Move { from, to } => from.ends_with("a.txt") && to.ends_with("b.txt"),
            _ => false,
        });
        assert!(has_move, "expected move op in patch: {:?}", patch);
        temp.close()?;
        Ok(())
    }
}
