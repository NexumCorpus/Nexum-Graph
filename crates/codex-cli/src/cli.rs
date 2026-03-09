//! CLI argument parsing via clap derive.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Project Codex — AI-native code coordination.
#[derive(Parser, Debug)]
#[command(name = "codex", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Compute semantic diff between two git refs.
    Diff {
        /// Base git ref (commit, branch, tag).
        ref_a: String,
        /// Target git ref (commit, branch, tag).
        ref_b: String,
        /// Path to the git repository (defaults to ".").
        #[arg(long)]
        repo_path: Option<PathBuf>,
        /// Output format: json, text, or github.
        #[arg(long, default_value = "text")]
        format: String,
    },
}
