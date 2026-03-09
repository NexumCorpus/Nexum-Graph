use clap::Parser;
use codex_lsp::{CodexLspConfig, build_service};
use std::path::PathBuf;
use tower_lsp::Server;

#[derive(Debug, Parser)]
#[command(name = "codex-lsp", version, about)]
struct Args {
    /// Repository root used for git reads and `.codex/` state files.
    #[arg(long)]
    repo_path: Option<PathBuf>,
    /// Base ref used for semantic diff and validation requests.
    #[arg(long, default_value = "HEAD~1")]
    base_ref: String,
    /// Poll interval in milliseconds for semantic event notifications.
    #[arg(long, default_value_t = 500)]
    event_poll_ms: u64,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let config = CodexLspConfig {
        repo_path: args.repo_path,
        base_ref: args.base_ref,
        event_poll_ms: args.event_poll_ms,
    };

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = build_service(config);
    Server::new(stdin, stdout, socket).serve(service).await;
}
