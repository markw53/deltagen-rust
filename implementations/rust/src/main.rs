use anyhow::Result;
use clap::{Parser, Subcommand};
use std::fs;

#[derive(Parser)]
#[command(name = "deltagen")]
#[command(about = "Compute and apply deterministic directory tree deltas", long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Compute {
        #[arg(long)]
        src: String,
        #[arg(long)]
        dst: String,
        #[arg(long)]
        out: String,
    },
    Apply {
        #[arg(long)]
        root: String,
        #[arg(long)]
        patch: String,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    Invert {
        #[arg(long)]
        patch: String,
        #[arg(long)]
        out: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Commands::Compute { src, dst, out } => {
            let src_snap = deltagen::snapshot::Snapshot::scan(&src)?;
            let dst_snap = deltagen::snapshot::Snapshot::scan(&dst)?;
            let patch = deltagen::delta::compute_delta(&src_snap, &dst_snap)?;
            let json = serde_json::to_string_pretty(&patch)?;
            fs::write(out, json)?;
        }
        Commands::Apply {
            root,
            patch,
            dry_run,
        } => {
            let s = fs::read_to_string(&patch)?;
            let patch_ops: Vec<deltagen::delta::Operation> = serde_json::from_str(&s)?;
            deltagen::apply::apply_patch(&root, &patch_ops, dry_run)?;
        }
        Commands::Invert { patch, out } => {
            let s = fs::read_to_string(&patch)?;
            let patch_ops: Vec<deltagen::delta::Operation> = serde_json::from_str(&s)?;
            let inv = deltagen::delta::invert_patch(&patch_ops)?;
            let json = serde_json::to_string_pretty(&inv)?;
            fs::write(out, json)?;
        }
    }

    Ok(())
}
