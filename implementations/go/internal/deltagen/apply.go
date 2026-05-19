package deltagen

import (
    "errors"
    "fmt"
    "io"
    "os"
    "path/filepath"
    "strings"
)

// ApplyPatch applies operations to root. If dryRun is true, it prints ops and validates only.
// It uses a simple backup directory to allow best-effort rollback on error.
func ApplyPatch(root string, patch []Operation, dryRun bool) error {
    if dryRun {
        for _, op := range patch {
            b, _ := jsonMarshal(op)
            fmt.Println(string(b))
        }
        return nil
    }
    // backup dir
    backup := filepath.Join(os.TempDir(), "deltagen_backup")
    if err := os.MkdirAll(backup, 0o700); err != nil {
        return err
    }
    applied := []Operation{}
    for _, op := range patch {
        if err := applyOne(root, op, backup); err != nil {
            // attempt rollback
            _ = rollback(root, backup)
            return fmt.Errorf("apply op %v failed: %w", op, err)
        }
        applied = append(applied, op)
    }
    // success: remove backup
    _ = os.RemoveAll(backup)
    return nil
}

func applyOne(root string, op Operation, backup string) error {
    switch op.Op {
    case "create_dir":
        p := filepath.Join(root, filepath.FromSlash(op.Path))
        return os.MkdirAll(p, 0o755)
    case "delete_dir":
        p := filepath.Join(root, filepath.FromSlash(op.Path))
        if _, err := os.Stat(p); err == nil {
            // backup
            if err := copyToBackup(p, backup, root); err != nil {
                return err
            }
            return os.RemoveAll(p)
        }
        return nil
    case "create_file":
        p := filepath.Join(root, filepath.FromSlash(op.Path))
        if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
            return err
        }
        // backup existing
        if _, err := os.Stat(p); err == nil {
            if err := copyToBackup(p, backup, root); err != nil {
                return err
            }
        }
        f, err := os.Create(p)
        if err != nil {
            return err
        }
        defer f.Close()
        if op.Content != "" {
            if _, err := f.WriteString(op.Content); err != nil {
                return err
            }
        }
        return nil
    case "delete_file":
        p := filepath.Join(root, filepath.FromSlash(op.Path))
        if _, err := os.Stat(p); err == nil {
            if err := copyToBackup(p, backup, root); err != nil {
                return err
            }
            return os.RemoveAll(p)
        }
        return nil
    case "modify_file":
        p := filepath.Join(root, filepath.FromSlash(op.Path))
        if _, err := os.Stat(p); err == nil {
            if err := copyToBackup(p, backup, root); err != nil {
                return err
            }
        } else {
            if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
                return err
            }
        }
        f, err := os.Create(p)
        if err != nil {
            return err
        }
        defer f.Close()
        if op.Content != "" {
            if _, err := f.WriteString(op.Content); err != nil {
                return err
            }
        }
        return nil
    case "move":
        src := filepath.Join(root, filepath.FromSlash(op.From))
        dst := filepath.Join(root, filepath.FromSlash(op.To))
        if _, err := os.Stat(src); err != nil {
            return fmt.Errorf("move source missing: %s", src)
        }
        if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
            return err
        }
        // backup dst if exists
        if _, err := os.Stat(dst); err == nil {
            if err := copyToBackup(dst, backup, root); err != nil {
                return err
            }
        }
        return os.Rename(src, dst)
    case "chmod":
        p := filepath.Join(root, filepath.FromSlash(op.Path))
        if op.Mode == nil {
            return errors.New("chmod missing mode")
        }
        return os.Chmod(p, os.FileMode(*op.Mode))
    case "utimes":
        // best-effort: skip precise atime/mtime handling for brevity
        return nil
    case "symlink":
        p := filepath.Join(root, filepath.FromSlash(op.Path))
        if _, err := os.Lstat(p); err == nil {
            if err := os.RemoveAll(p); err != nil {
                return err
            }
        }
        return os.Symlink(op.Target, p)
    default:
        return fmt.Errorf("unknown op: %s", op.Op)
    }
}

func copyToBackup(path, backup, root string) error {
    rel, err := filepath.Rel(root, path)
    if err != nil {
        return err
    }
    dest := filepath.Join(backup, rel)
    if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
        return err
    }
    info, err := os.Lstat(path)
    if err != nil {
        return err
    }
    if info.Mode().IsDir() {
        return copyDir(path, dest)
    }
    // file or symlink
    srcf, err := os.Open(path)
    if err != nil {
        return err
    }
    defer srcf.Close()
    dstf, err := os.Create(dest)
    if err != nil {
        return err
    }
    defer dstf.Close()
    _, err = io.Copy(dstf, srcf)
    return err
}

func copyDir(src, dst string) error {
    return filepath.WalkDir(src, func(p string, d os.DirEntry, err error) error {
        if err != nil {
            return err
        }
        rel, _ := filepath.Rel(src, p)
        dest := filepath.Join(dst, rel)
        if d.IsDir() {
            return os.MkdirAll(dest, 0o755)
        }
        // file
        if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
            return err
        }
        sf, err := os.Open(p)
        if err != nil {
            return err
        }
        defer sf.Close()
        df, err := os.Create(dest)
        if err != nil {
            return err
        }
        defer df.Close()
        _, err = io.Copy(df, sf)
        return err
    })
}

func rollback(root, backup string) error {
    if _, err := os.Stat(backup); os.IsNotExist(err) {
        return nil
    }
    return filepath.WalkDir(backup, func(p string, d os.DirEntry, err error) error {
        if err != nil {
            return err
        }
        if p == backup {
            return nil
        }
        rel, _ := filepath.Rel(backup, p)
        dest := filepath.Join(root, rel)
        if d.IsDir() {
            return os.MkdirAll(dest, 0o755)
        }
        // file
        if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
            return err
        }
        sf, err := os.Open(p)
        if err != nil {
            return err
        }
        defer sf.Close()
        df, err := os.Create(dest)
        if err != nil {
            return err
        }
        defer df.Close()
        _, err = io.Copy(df, sf)
        return err
    })
}
