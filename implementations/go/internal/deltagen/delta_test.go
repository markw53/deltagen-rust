package deltagen_test

import (
    "encoding/json"
    "os"
    "path/filepath"
    "testing"
    "time"

    "github.com/you/deltagen/internal/deltagen"
)

func writeFile(t *testing.T, path, content string) {
    t.Helper()
    if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
        t.Fatalf("mkdir: %v", err)
    }
    if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
        t.Fatalf("write: %v", err)
    }
}

func TestIdenticalTreesProduceEmptyPatch(t *testing.T) {
    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    os.MkdirAll(src, 0o755)
    os.MkdirAll(dst, 0o755)

    writeFile(t, filepath.Join(src, "a.txt"), "hello")
    writeFile(t, filepath.Join(dst, "a.txt"), "hello")

    snapSrc, err := deltagen.Scan(src)
    if err != nil {
        t.Fatal(err)
    }
    snapDst, err := deltagen.Scan(dst)
    if err != nil {
        t.Fatal(err)
    }

    patch, err := deltagen.ComputeDelta(snapSrc, snapDst)
    if err != nil {
        t.Fatal(err)
    }
    if len(patch) != 0 {
        t.Fatalf("expected empty patch, got %v", patch)
    }
}

func TestRenameDetectedAsMove(t *testing.T) {
    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    os.MkdirAll(src, 0o755)
    os.MkdirAll(dst, 0o755)

    writeFile(t, filepath.Join(src, "a.txt"), "same")
    writeFile(t, filepath.Join(dst, "b.txt"), "same")

    snapSrc, _ := deltagen.Scan(src)
    snapDst, _ := deltagen.Scan(dst)

    patch, err := deltagen.ComputeDelta(snapSrc, snapDst)
    if err != nil {
        t.Fatal(err)
    }

    found := false
    for _, op := range patch {
        if op.Op == "move" && op.From == "a.txt" && op.To == "b.txt" {
            found = true
            break
        }
    }
    if !found {
        b, _ := json.MarshalIndent(patch, "", "  ")
        t.Fatalf("expected move a.txt -> b.txt, got %s", b)
    }
}

func TestSymlinkTargetChange(t *testing.T) {
    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    os.MkdirAll(src, 0o755)
    os.MkdirAll(dst, 0o755)

    writeFile(t, filepath.Join(src, "a.txt"), "hello")
    writeFile(t, filepath.Join(dst, "b.txt"), "hello")

    // symlink src/link -> a.txt
    if err := os.Symlink("a.txt", filepath.Join(src, "link")); err != nil {
        t.Skip("symlink not supported on this platform")
    }

    // symlink dst/link -> b.txt
    if err := os.Symlink("b.txt", filepath.Join(dst, "link")); err != nil {
        t.Skip("symlink not supported on this platform")
    }

    snapSrc, _ := deltagen.Scan(src)
    snapDst, _ := deltagen.Scan(dst)

    patch, err := deltagen.ComputeDelta(snapSrc, snapDst)
    if err != nil {
        t.Fatal(err)
    }

    found := false
    for _, op := range patch {
        if op.Op == "symlink" && op.Path == "link" && op.Target == "b.txt" {
            found = true
        }
    }
    if !found {
        t.Fatalf("expected symlink update, got %v", patch)
    }
}

func TestPermissionOnlyChange(t *testing.T) {
    // Only works on Unix
    if os.PathSeparator == '\\' {
        t.Skip("permissions test skipped on Windows")
    }

    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    os.MkdirAll(src, 0o755)
    os.MkdirAll(dst, 0o755)

    srcFile := filepath.Join(src, "a.txt")
    dstFile := filepath.Join(dst, "a.txt")

    writeFile(t, srcFile, "content")
    writeFile(t, dstFile, "content")

    os.Chmod(srcFile, 0o644)
    os.Chmod(dstFile, 0o600)

    snapSrc, _ := deltagen.Scan(src)
    snapDst, _ := deltagen.Scan(dst)

    patch, err := deltagen.ComputeDelta(snapSrc, snapDst)
    if err != nil {
        t.Fatal(err)
    }

    found := false
    for _, op := range patch {
        if op.Op == "chmod" && op.Path == "a.txt" {
            found = true
        }
    }
    if !found {
        t.Fatalf("expected chmod op, got %v", patch)
    }
}

func TestTimestampOnlyChange(t *testing.T) {
    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    os.MkdirAll(src, 0o755)
    os.MkdirAll(dst, 0o755)

    srcFile := filepath.Join(src, "a.txt")
    dstFile := filepath.Join(dst, "a.txt")

    writeFile(t, srcFile, "content")
    writeFile(t, dstFile, "content")

    // Set different mtimes
    now := time.Now()
    later := now.Add(1 * time.Minute)

    os.Chtimes(srcFile, now, now)
    os.Chtimes(dstFile, later, later)

    snapSrc, _ := deltagen.Scan(src)
    snapDst, _ := deltagen.Scan(dst)

    patch, err := deltagen.ComputeDelta(snapSrc, snapDst)
    if err != nil {
        t.Fatal(err)
    }

    found := false
    for _, op := range patch {
        if op.Op == "utimes" && op.Path == "a.txt" {
            found = true
        }
    }
    if !found {
        t.Fatalf("expected utimes op, got %v", patch)
    }
}

func TestSwapIdenticalFiles(t *testing.T) {
    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    os.MkdirAll(src, 0o755)
    os.MkdirAll(dst, 0o755)

    writeFile(t, filepath.Join(src, "a.txt"), "X")
    writeFile(t, filepath.Join(src, "b.txt"), "Y")

    writeFile(t, filepath.Join(dst, "a.txt"), "Y")
    writeFile(t, filepath.Join(dst, "b.txt"), "X")

    snapSrc, _ := deltagen.Scan(src)
    snapDst, _ := deltagen.Scan(dst)

    patch, err := deltagen.ComputeDelta(snapSrc, snapDst)
    if err != nil {
        t.Fatal(err)
    }

    moveCount := 0
    modCount := 0
    for _, op := range patch {
        if op.Op == "move" {
            moveCount++
        }
        if op.Op == "modify_file" {
            modCount++
        }
    }

    if moveCount+modCount < 2 {
        t.Fatalf("expected at least two ops for swap, got %v", patch)
    }
}

func TestNestedMoves(t *testing.T) {
    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    os.MkdirAll(filepath.Join(src, "dir1/sub"), 0o755)
    os.MkdirAll(filepath.Join(dst, "dir2/sub"), 0o755)

    writeFile(t, filepath.Join(src, "dir1/x.txt"), "A")
    writeFile(t, filepath.Join(src, "dir1/sub/y.txt"), "B")

    writeFile(t, filepath.Join(dst, "dir2/x.txt"), "A")
    writeFile(t, filepath.Join(dst, "dir2/sub/y.txt"), "B")

    snapSrc, _ := deltagen.Scan(src)
    snapDst, _ := deltagen.Scan(dst)

    patch, err := deltagen.ComputeDelta(snapSrc, snapDst)
    if err != nil {
        t.Fatal(err)
    }

    deleteCount := 0
    for _, op := range patch {
        if op.Op == "delete_file" || op.Op == "delete_dir" {
            deleteCount++
        }
    }

    if deleteCount > 3 {
        t.Fatalf("too many deletes for nested move, patch=%v", patch)
    }
}

func TestLargeTreeSmoke(t *testing.T) {
    t.Skip("enable manually for performance testing")

    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    os.MkdirAll(src, 0o755)
    os.MkdirAll(dst, 0o755)

    for i := 0; i < 50000; i++ {
        name := filepath.Join(src, "f"+fmtInt(i)+".txt")
        writeFile(t, name, "content")
        if i%2 == 0 {
            writeFile(t, filepath.Join(dst, "f"+fmtInt(i)+".txt"), "content")
        }
    }

    snapSrc, _ := deltagen.Scan(src)
    snapDst, _ := deltagen.Scan(dst)

    p1, _ := deltagen.ComputeDelta(snapSrc, snapDst)
    p2, _ := deltagen.ComputeDelta(snapSrc, snapDst)

    j1, _ := json.Marshal(p1)
    j2, _ := json.Marshal(p2)

    if string(j1) != string(j2) {
        t.Fatalf("patch not deterministic")
    }
}

func fmtInt(i int) string {
    return fmt.Sprintf("%05d", i)
}
