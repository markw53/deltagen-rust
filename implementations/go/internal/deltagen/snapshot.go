package deltagen

import (
    "os"
    "path/filepath"
    "sort"
    "time"
)

// EntryType enumerates file/dir/symlink
type EntryType string

const (
    File    EntryType = "file"
    Dir     EntryType = "dir"
    Symlink EntryType = "symlink"
)

// Entry is a single snapshot record
type Entry struct {
    Path       string    `json:"path"`        // relative POSIX-style
    Type       EntryType `json:"type"`        // file|dir|symlink
    Size       int64     `json:"size,omitempty"`
    Mode       os.FileMode `json:"mode,omitempty"`
    Mtime      int64     `json:"mtime,omitempty"` // unix seconds
    Hash       string    `json:"hash,omitempty"`  // sha256 hex for files
    LinkTarget string    `json:"link_target,omitempty"`
}

// Snapshot holds root and entries
type Snapshot struct {
    Root    string  `json:"root"`
    Entries []Entry `json:"entries"`
}

// Scan walks a directory and returns a Snapshot. It computes SHA256 for files.
func Scan(root string) (Snapshot, error) {
    entries := []Entry{}
    err := filepath.WalkDir(root, func(p string, d os.DirEntry, err error) error {
        if err != nil {
            return err
        }
        if p == root {
            return nil
        }
        rel, err := filepath.Rel(root, p)
        if err != nil {
            return err
        }
        info, err := d.Info()
        if err != nil {
            return err
        }
        if d.Type()&os.ModeSymlink != 0 {
            // symlink
            target, _ := os.Readlink(p)
            entries = append(entries, Entry{
                Path:       filepath.ToSlash(rel),
                Type:       Symlink,
                LinkTarget: target,
            })
            return nil
        }
        if info.IsDir() {
            entries = append(entries, Entry{
                Path: filepath.ToSlash(rel),
                Type: Dir,
                Mode: info.Mode(),
                Mtime: info.ModTime().Unix(),
            })
            return nil
        }
        // file
        h, err := computeSHA256(p)
        if err != nil {
            return err
        }
        entries = append(entries, Entry{
            Path:  filepath.ToSlash(rel),
            Type:  File,
            Size:  info.Size(),
            Mode:  info.Mode(),
            Mtime: info.ModTime().Unix(),
            Hash:  h,
        })
        return nil
    })
    if err != nil {
        return Snapshot{}, err
    }
    // deterministic ordering
    sort.Slice(entries, func(i, j int) bool { return entries[i].Path < entries[j].Path })
    return Snapshot{Root: root, Entries: entries}, nil
}
