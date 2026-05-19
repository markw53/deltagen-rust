package deltagen

import (
    "sort"
)

// Operation types and JSON tags
type Operation struct {
    Op          string  `json:"op"`
    Path        string  `json:"path,omitempty"`
    From        string  `json:"from,omitempty"`
    To          string  `json:"to,omitempty"`
    ContentHash string  `json:"content_hash,omitempty"`
    Mode        *uint32 `json:"mode,omitempty"`
    Mtime       *int64  `json:"mtime,omitempty"`
    OldHash     string  `json:"old_hash,omitempty"`
    NewHash     string  `json:"new_hash,omitempty"`
    Content     string  `json:"content,omitempty"`
    Target      string  `json:"target,omitempty"`
}

// ComputeDelta produces a deterministic patch from src -> dst
func ComputeDelta(src Snapshot, dst Snapshot) ([]Operation, error) {
    srcMap := map[string]Entry{}
    dstMap := map[string]Entry{}
    for _, e := range src.Entries {
        srcMap[e.Path] = e
    }
    for _, e := range dst.Entries {
        dstMap[e.Path] = e
    }

    var ops []Operation
    var metadataOps []Operation

    // collect src-only and dst-only
    srcOnly := []string{}
    dstOnly := []string{}
    for p := range srcMap {
        if _, ok := dstMap[p]; !ok {
            srcOnly = append(srcOnly, p)
        }
    }
    for p := range dstMap {
        if _, ok := srcMap[p]; !ok {
            dstOnly = append(dstOnly, p)
        }
    }

    // same-path handling
    for p, se := range srcMap {
        if de, ok := dstMap[p]; ok {
            if se.Type != de.Type {
                // type changed: delete old, create new
                switch se.Type {
                case File:
                    ops = append(ops, Operation{Op: "delete_file", Path: p})
                case Dir:
                    ops = append(ops, Operation{Op: "delete_dir", Path: p})
                case Symlink:
                    ops = append(ops, Operation{Op: "delete_file", Path: p})
                }
                switch de.Type {
                case File:
                    ops = append(ops, Operation{Op: "create_file", Path: p, ContentHash: de.Hash})
                case Dir:
                    ops = append(ops, Operation{Op: "create_dir", Path: p})
                case Symlink:
                    ops = append(ops, Operation{Op: "symlink", Path: p, Target: de.LinkTarget})
                }
                continue
            }
            // same type
            if se.Type == File {
                if se.Hash != de.Hash {
                    metadataOps = append(metadataOps, Operation{Op: "modify_file", Path: p, OldHash: se.Hash, NewHash: de.Hash})
                } else {
                    if se.Mode != de.Mode {
                        m := uint32(de.Mode)
                        metadataOps = append(metadataOps, Operation{Op: "chmod", Path: p, Mode: &m})
                    }
                    if se.Mtime != de.Mtime {
                        mt := de.Mtime
                        metadataOps = append(metadataOps, Operation{Op: "utimes", Path: p, Mtime: &mt})
                    }
                }
            } else if se.Type == Dir {
                if se.Mode != de.Mode {
                    m := uint32(de.Mode)
                    metadataOps = append(metadataOps, Operation{Op: "chmod", Path: p, Mode: &m})
                }
            } else if se.Type == Symlink {
                if se.LinkTarget != de.LinkTarget {
                    metadataOps = append(metadataOps, Operation{Op: "symlink", Path: p, Target: de.LinkTarget})
                }
            }
        }
    }

    // build hash maps for move detection
    srcHash := map[string][]string{}
    dstHash := map[string][]string{}
    srcSize := map[string]int64{}
    for _, p := range srcOnly {
        if e, ok := srcMap[p]; ok && e.Type == File && e.Hash != "" {
            srcHash[e.Hash] = append(srcHash[e.Hash], p)
            srcSize[p] = e.Size
        }
    }
    for _, p := range dstOnly {
        if e, ok := dstMap[p]; ok && e.Type == File && e.Hash != "" {
            dstHash[e.Hash] = append(dstHash[e.Hash], p)
        }
    }

    // candidate pairs
    type pair struct {
        from string
        to   string
        size int64
    }
    var pairs []pair
    for h, dsts := range dstHash {
        if srcs, ok := srcHash[h]; ok {
            for _, d := range dsts {
                for _, s := range srcs {
                    pairs = append(pairs, pair{from: s, to: d, size: srcSize[s]})
                }
            }
        }
    }
    // sort by size desc, then from lexicographic
    sort.Slice(pairs, func(i, j int) bool {
        if pairs[i].size != pairs[j].size {
            return pairs[i].size > pairs[j].size
        }
        if pairs[i].from != pairs[j].from {
            return pairs[i].from < pairs[j].from
        }
        return pairs[i].to < pairs[j].to
    })

    usedSrc := map[string]bool{}
    usedDst := map[string]bool{}
    var selected []pair
    for _, pr := range pairs {
        if usedSrc[pr.from] || usedDst[pr.to] {
            continue
        }
        usedSrc[pr.from] = true
        usedDst[pr.to] = true
        selected = append(selected, pr)
    }

    // emit moves sorted by from
    sort.Slice(selected, func(i, j int) bool { return selected[i].from < selected[j].from })
    for _, s := range selected {
        ops = append(ops, Operation{Op: "move", From: s.from, To: s.to})
    }

    // remove moved from srcOnly/dstOnly
    filter := func(list []string, used map[string]bool) []string {
        out := make([]string, 0, len(list))
        for _, p := range list {
            if !used[p] {
                out = append(out, p)
            }
        }
        return out
    }
    srcOnly = filter(srcOnly, usedSrc)
    dstOnly = filter(dstOnly, usedDst)

    // deletes: deepest-first
    sort.Slice(srcOnly, func(i, j int) bool {
        di := depth(srcOnly[i])
        dj := depth(srcOnly[j])
        if di != dj {
            return di > dj
        }
        return srcOnly[i] < srcOnly[j]
    })
    for _, p := range srcOnly {
        if e, ok := srcMap[p]; ok {
            switch e.Type {
            case File:
                ops = append(ops, Operation{Op: "delete_file", Path: p})
            case Dir:
                ops = append(ops, Operation{Op: "delete_dir", Path: p})
            case Symlink:
                ops = append(ops, Operation{Op: "delete_file", Path: p})
            }
        }
    }

    // creates: parents-first
    sort.Slice(dstOnly, func(i, j int) bool {
        di := depth(dstOnly[i])
        dj := depth(dstOnly[j])
        if di != dj {
            return di < dj
        }
        return dstOnly[i] < dstOnly[j]
    })
    for _, p := range dstOnly {
        if e, ok := dstMap[p]; ok {
            switch e.Type {
            case Dir:
                ops = append(ops, Operation{Op: "create_dir", Path: p})
            case File:
                var m *uint32
                if e.Mode != 0 {
                    mm := uint32(e.Mode)
                    m = &mm
                }
                var mt *int64
                if e.Mtime != 0 {
                    mt = &e.Mtime
                }
                ops = append(ops, Operation{Op: "create_file", Path: p, ContentHash: e.Hash, Mode: m, Mtime: mt})
            case Symlink:
                ops = append(ops, Operation{Op: "symlink", Path: p, Target: e.LinkTarget})
            }
        }
    }

    // append metadata ops deterministically (sort by op then path)
    sort.Slice(metadataOps, func(i, j int) bool {
        if metadataOps[i].Op != metadataOps[j].Op {
            return metadataOps[i].Op < metadataOps[j].Op
        }
        return metadataOps[i].Path < metadataOps[j].Path
    })
    ops = append(ops, metadataOps...)
    return ops, nil
}

func depth(p string) int {
    if p == "" {
        return 0
    }
    cnt := 0
    for _, r := range p {
        if r == '/' || r == '\\' {
            cnt++
        }
    }
    return cnt
}

// InvertPatch returns a best-effort inverse patch
func InvertPatch(patch []Operation) ([]Operation, error) {
    var inv []Operation
    for i := len(patch) - 1; i >= 0; i-- {
        op := patch[i]
        switch op.Op {
        case "create_file":
            inv = append(inv, Operation{Op: "delete_file", Path: op.Path})
        case "delete_file":
            inv = append(inv, Operation{Op: "create_file", Path: op.Path})
        case "move":
            inv = append(inv, Operation{Op: "move", From: op.To, To: op.From})
        case "modify_file":
            inv = append(inv, Operation{Op: "modify_file", Path: op.Path, OldHash: op.NewHash, NewHash: op.OldHash})
        case "create_dir":
            inv = append(inv, Operation{Op: "delete_dir", Path: op.Path})
        case "delete_dir":
            inv = append(inv, Operation{Op: "create_dir", Path: op.Path})
        case "chmod":
            inv = append(inv, Operation{Op: "chmod", Path: op.Path, Mode: op.Mode})
        case "utimes":
            inv = append(inv, Operation{Op: "utimes", Path: op.Path, Mtime: op.Mtime})
        case "symlink":
            inv = append(inv, Operation{Op: "delete_file", Path: op.Path})
        }
    }
    return inv, nil
}
