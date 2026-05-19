package main

import (
    "encoding/json"
    "flag"
    "fmt"
    "os"

    "github.com/you/deltagen/internal/deltagen"
)

func usage() {
    fmt.Println("deltagen compute --src <src> --dst <dst> --out <patch.json>")
    fmt.Println("deltagen apply --root <root> --patch <patch.json> [--dry-run]")
    fmt.Println("deltagen invert --patch <patch.json> --out <inverse.json>")
    fmt.Println("deltagen validate --src <src> --patch <patch.json> --dst <dst>")
}

func main() {
    if len(os.Args) < 2 {
        usage()
        os.Exit(2)
    }
    cmd := os.Args[1]
    switch cmd {
    case "compute":
        fs := flag.NewFlagSet("compute", flag.ExitOnError)
        src := fs.String("src", "", "source directory")
        dst := fs.String("dst", "", "destination directory")
        out := fs.String("out", "patch.json", "output patch file")
        fs.Parse(os.Args[2:])
        if *src == "" || *dst == "" {
            fmt.Println("src and dst required")
            usage()
            os.Exit(2)
        }
        srcSnap, err := deltagen.Scan(*src)
        if err != nil {
            fmt.Fprintln(os.Stderr, "scan src:", err)
            os.Exit(1)
        }
        dstSnap, err := deltagen.Scan(*dst)
        if err != nil {
            fmt.Fprintln(os.Stderr, "scan dst:", err)
            os.Exit(1)
        }
        patch, err := deltagen.ComputeDelta(srcSnap, dstSnap)
        if err != nil {
            fmt.Fprintln(os.Stderr, "compute delta:", err)
            os.Exit(1)
        }
        b, _ := json.MarshalIndent(patch, "", "  ")
        if err := os.WriteFile(*out, b, 0o644); err != nil {
            fmt.Fprintln(os.Stderr, "write patch:", err)
            os.Exit(1)
        }
    case "apply":
        fs := flag.NewFlagSet("apply", flag.ExitOnError)
        root := fs.String("root", "", "root directory to apply patch")
        patchFile := fs.String("patch", "", "patch json file")
        dry := fs.Bool("dry-run", false, "dry run")
        fs.Parse(os.Args[2:])
        if *root == "" || *patchFile == "" {
            usage()
            os.Exit(2)
        }
        b, err := os.ReadFile(*patchFile)
        if err != nil {
            fmt.Fprintln(os.Stderr, "read patch:", err)
            os.Exit(1)
        }
        var patch []deltagen.Operation
        if err := json.Unmarshal(b, &patch); err != nil {
            fmt.Fprintln(os.Stderr, "parse patch:", err)
            os.Exit(1)
        }
        if err := deltagen.ApplyPatch(*root, patch, *dry); err != nil {
            fmt.Fprintln(os.Stderr, "apply patch:", err)
            os.Exit(1)
        }
    case "invert":
        fs := flag.NewFlagSet("invert", flag.ExitOnError)
        patchFile := fs.String("patch", "", "patch json file")
        out := fs.String("out", "inverse.json", "output inverse patch")
        fs.Parse(os.Args[2:])
        if *patchFile == "" {
            usage()
            os.Exit(2)
        }
        b, err := os.ReadFile(*patchFile)
        if err != nil {
            fmt.Fprintln(os.Stderr, "read patch:", err)
            os.Exit(1)
        }
        var patch []deltagen.Operation
        if err := json.Unmarshal(b, &patch); err != nil {
            fmt.Fprintln(os.Stderr, "parse patch:", err)
            os.Exit(1)
        }
        inv, err := deltagen.InvertPatch(patch)
        if err != nil {
            fmt.Fprintln(os.Stderr, "invert:", err)
            os.Exit(1)
        }
        b2, _ := json.MarshalIndent(inv, "", "  ")
        if err := os.WriteFile(*out, b2, 0o644); err != nil {
            fmt.Fprintln(os.Stderr, "write inverse:", err)
            os.Exit(1)
        }
    case "validate":
        fs := flag.NewFlagSet("validate", flag.ExitOnError)
        src := fs.String("src", "", "source directory")
        patchFile := fs.String("patch", "", "patch json file")
        dst := fs.String("dst", "", "destination directory")
        fs.Parse(os.Args[2:])
        if *src == "" || *patchFile == "" || *dst == "" {
            usage()
            os.Exit(2)
        }
        srcSnap, err := deltagen.Scan(*src)
        if err != nil {
            fmt.Fprintln(os.Stderr, "scan src:", err)
            os.Exit(1)
        }
        b, err := os.ReadFile(*patchFile)
        if err != nil {
            fmt.Fprintln(os.Stderr, "read patch:", err)
            os.Exit(1)
        }
        var patch []deltagen.Operation
        if err := json.Unmarshal(b, &patch); err != nil {
            fmt.Fprintln(os.Stderr, "parse patch:", err)
            os.Exit(1)
        }
        ok, err := deltagen.ValidatePatch(srcSnap, patch, *dst)
        if err != nil {
            fmt.Fprintln(os.Stderr, "validate:", err)
            os.Exit(1)
        }
        if ok {
            fmt.Println("patch is valid")
        } else {
            fmt.Println("patch is NOT valid")
            os.Exit(1)
        }
    default:
        usage()
        os.Exit(2)
    }
}
