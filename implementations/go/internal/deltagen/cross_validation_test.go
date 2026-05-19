package deltagen_test

import (
    "encoding/json"
    "os"
    "os/exec"
    "path/filepath"
    "testing"

    "github.com/you/deltagen/internal/deltagen"
)

func TestGoComputes_RustApplies(t *testing.T) {
    // --- Setup temp dirs ---
    dir := t.TempDir()
    src := filepath.Join(dir, "src")
    dst := filepath.Join(dir, "dst")
    out := filepath.Join(dir, "patch.json")
    final := filepath.Join(dir, "final")

    must(os.MkdirAll(src, 0o755))
    must(os.MkdirAll(dst, 0o755))
    must(os.MkdirAll(final, 0o755))

    // --- Create example trees ---
    write(t, filepath.Join(src, "a.txt"), "hello")
    write(t, filepath.Join(src, "dir/x.txt"), "one")

    write(t, filepath.Join(dst, "b.txt"), "hello") // moved
    write(t, filepath.Join(dst, "dir/x.txt"), "two") // modified

    // --- Step 1: Go computes patch ---
    goBin := goBinaryPath(t)
    cmd := exec.Command(goBin,
        "compute",
        "--src", src,
        "--dst", dst,
        "--out", out,
    )
    run(t, cmd)

    // --- Step 2: Rust applies patch ---
    rustBin := rustBinaryPath(t)
    cmd = exec.Command(rustBin,
        "apply",
        "--root", final,
        "--patch", out,
    )
    run(t, cmd)

    // --- Step 3: Snapshot final tree using Go ---
    finalSnap, err := deltagen.Scan(final)
    if err != nil {
        t.Fatalf("scan final: %v", err)
    }

    // --- Step 4: Snapshot expected dst tree using Go ---
    dstSnap, err := deltagen.Scan(dst)
    if err != nil {
        t.Fatalf("scan dst: %v", err)
    }

    // --- Step 5: Compare snapshots ---
    fb, _ := json.Marshal(finalSnap)
    db, _ := json.Marshal(dstSnap)

    if string(fb) != string(db) {
        t.Fatalf("final tree does not match expected.\nFinal: %s\nExpected: %s", fb, db)
    }
}

// --- Helpers ---

func write(t *testing.T, path, content string) {
    t.Helper()
    must(os.MkdirAll(filepath.Dir(path), 0o755))
    must(os.WriteFile(path, []byte(content), 0o644))
}

func must(err error) {
    if err != nil {
        panic(err)
    }
}

func run(t *testing.T, cmd *exec.Cmd) {
    t.Helper()
    out, err := cmd.CombinedOutput()
    if err != nil {
        t.Fatalf("command failed: %v\nOutput:\n%s", err, string(out))
    }
}

func goBinaryPath(t *testing.T) string {
    // Path to Go binary built from implementations/go
    path := "../../go/cmd/deltagen/deltagen"
    if _, err := os.Stat(path); err != nil {
        t.Fatalf("Go binary not found at %s. Run: go build ./...", path)
    }
    return path
}

func rustBinaryPath(t *testing.T) string {
    // Path to Rust binary built from implementations/rust
    path := "../../rust/target/debug/deltagen"
    if _, err := os.Stat(path); err != nil {
        t.Fatalf("Rust binary not found at %s. Run: cargo build", path)
    }
    return path
}
