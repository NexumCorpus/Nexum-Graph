// codex-cli: CLI binary for Project Codex
// Phase 0: `codex diff <ref-a> <ref-b>`

use clap::Parser;
use codex_cli::cli::{Cli, Commands};
use codex_cli::{output, pipeline};

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Diff {
            ref_a,
            ref_b,
            repo_path,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match pipeline::run_diff(repo, &ref_a, &ref_b) {
                Ok(diff) => {
                    let out = output::format_diff(&diff, &format);
                    println!("{out}");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
