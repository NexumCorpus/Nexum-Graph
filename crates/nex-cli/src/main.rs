// nex-cli: CLI binary for Nexum Graph
// Phase 0: `nex diff <ref-a> <ref-b>`
// Phase 1: `nex check <branch-a> <branch-b>`
// Phase 2: `nex lock`, `nex unlock`, `nex locks`, `nex validate`
// Phase 3: `nex log`, `nex rollback`

use clap::Parser;
use nex_cli::cli::{AuditCommands, AuthCommands, Cli, Commands, GithubCommands};
use nex_cli::{
    audit_pipeline, auth_pipeline, check_pipeline, coordination_pipeline, demo_pipeline,
    eventlog_pipeline, github_pipeline, output, serve_pipeline, start_pipeline,
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Demo {
            base,
            head,
            repo_path,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
            match demo_pipeline::run_demo(repo, &base, &head).await {
                Ok(report) => {
                    let out = output::format_demo_report(&report, &format);
                    println!("{out}");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Start {
            base,
            head,
            repo_path,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
            match start_pipeline::run_start(repo, &base, &head).await {
                Ok(report) => {
                    let out = output::format_start_report(&report, &format);
                    println!("{out}");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Diff {
            ref_a,
            ref_b,
            repo_path,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match nex_cli::pipeline::run_diff(repo, &ref_a, &ref_b) {
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
        Commands::Check {
            branch_a,
            branch_b,
            repo_path,
            format,
            install_hook,
            force,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            if install_hook {
                match check_pipeline::install_check_hook(repo, force) {
                    Ok(result) => {
                        let out = output::format_check_hook_install_result(&result, &format);
                        println!("{out}");
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                let (Some(branch_a), Some(branch_b)) = (branch_a.as_deref(), branch_b.as_deref())
                else {
                    eprintln!(
                        "error: `nex check` requires <branch-a> and <branch-b>, or --install-hook"
                    );
                    std::process::exit(1);
                };

                match check_pipeline::run_check(repo, branch_a, branch_b) {
                    Ok(report) => {
                        let exit = report.exit_code();
                        let out = output::format_report(&report, &format);
                        println!("{out}");
                        std::process::exit(exit);
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
        Commands::Lock {
            agent_name,
            target_name,
            kind,
            repo_path,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match coordination_pipeline::run_lock(repo, &agent_name, &target_name, &kind) {
                Ok(result) => {
                    let out =
                        output::format_lock_result(&result, &agent_name, &target_name, &format);
                    println!("{out}");
                    if matches!(result, nex_core::LockResult::Denied { .. }) {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Unlock {
            agent_name,
            target_name,
            repo_path,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match coordination_pipeline::run_unlock(repo, &agent_name, &target_name) {
                Ok(()) => println!("Lock released."),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Locks { repo_path, format } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match coordination_pipeline::run_locks(repo) {
                Ok(entries) => {
                    let out = output::format_locks(&entries, &format);
                    println!("{out}");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Validate {
            agent_name,
            base,
            repo_path,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match coordination_pipeline::run_validate(repo, &agent_name, &base) {
                Ok(report) => {
                    let exit = report.exit_code();
                    let out = output::format_validation_report(&report, &format);
                    println!("{out}");
                    std::process::exit(exit);
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Log {
            repo_path,
            intent_id,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match eventlog_pipeline::run_log(repo, intent_id.as_deref()).await {
                Ok(events) => {
                    let out = output::format_event_log(&events, &format);
                    println!("{out}");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Rollback {
            intent_id,
            agent_name,
            repo_path,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match eventlog_pipeline::run_rollback(repo, &intent_id, &agent_name).await {
                Ok(outcome) => {
                    let out = output::format_rollback_outcome(&outcome, &format);
                    println!("{out}");
                    if !outcome.is_clean() {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Replay {
            to,
            repo_path,
            format,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            match eventlog_pipeline::run_replay(repo, &to).await {
                Ok(units) => {
                    let out = output::format_replay_state(&units, &format);
                    println!("{out}");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Serve {
            host,
            port,
            auth_token,
            agent_tokens,
            auth_config,
            allow_insecure_remote,
            repo_path,
        } => {
            let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));

            if let Err(e) = serve_pipeline::run_serve(
                repo,
                &host,
                port,
                auth_token,
                agent_tokens,
                auth_config,
                allow_insecure_remote,
            )
            .await
            {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Github { command } => match command {
            GithubCommands::Init {
                repo_path,
                workflow_name,
                gate_mode,
                force,
                no_pr_comment,
                no_sarif,
                format,
            } => {
                let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
                match github_pipeline::run_github_init(
                    repo,
                    &workflow_name,
                    &gate_mode,
                    !no_pr_comment,
                    !no_sarif,
                    force,
                ) {
                    Ok(result) => {
                        let out = output::format_github_init_result(&result, &format);
                        println!("{out}");
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            GithubCommands::Status {
                repo_path,
                require_managed,
                require_current,
                min_gate_mode,
                require_pr_comment,
                require_sarif,
                format,
            } => {
                let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
                match github_pipeline::run_github_status(repo) {
                    Ok(result) => {
                        let out = output::format_github_status(&result, &format);
                        println!("{out}");
                        if let Err(e) = github_pipeline::verify_github_status(
                            &result,
                            require_managed,
                            require_current,
                            min_gate_mode.as_deref(),
                            require_pr_comment,
                            require_sarif,
                        ) {
                            eprintln!("error: {e}");
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Auth { command } => match command {
            AuthCommands::Init {
                agents,
                shared,
                force,
                repo_path,
                auth_config,
                format,
            } => {
                let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
                match auth_pipeline::init_auth_config(repo, auth_config, &agents, shared, force) {
                    Ok(result) => {
                        let out = output::format_auth_init_result(&result, &format);
                        println!("{out}");
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            AuthCommands::Issue {
                agent_name,
                shared,
                repo_path,
                auth_config,
                format,
            } => {
                let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
                let target = match (shared, agent_name) {
                    (true, None) => auth_pipeline::AuthIssueTarget::Shared,
                    (false, Some(agent_name)) => auth_pipeline::AuthIssueTarget::Agent(agent_name),
                    (true, Some(_)) => {
                        eprintln!(
                            "error: use either `nex auth issue --shared` or `nex auth issue <agent>`"
                        );
                        std::process::exit(1);
                    }
                    (false, None) => {
                        eprintln!("error: `nex auth issue` requires an agent name or --shared");
                        std::process::exit(1);
                    }
                };

                match auth_pipeline::issue_auth_token(repo, auth_config, target) {
                    Ok(result) => {
                        let out = output::format_auth_issue_result(&result, &format);
                        println!("{out}");
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            AuthCommands::Revoke {
                token,
                repo_path,
                auth_config,
                format,
            } => {
                let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
                match auth_pipeline::revoke_auth_token(repo, auth_config, &token) {
                    Ok(result) => {
                        let out = output::format_auth_revoke_result(&result, &format);
                        println!("{out}");
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            AuthCommands::Status {
                repo_path,
                auth_config,
                format,
            } => {
                let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
                match auth_pipeline::auth_status(repo, auth_config) {
                    Ok(result) => {
                        let out = output::format_auth_status(&result, &format);
                        println!("{out}");
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Audit { command } => match command {
            AuditCommands::Verify {
                repo_path,
                audit_log,
                format,
            } => {
                let repo = repo_path.as_deref().unwrap_or(std::path::Path::new("."));
                match audit_pipeline::verify_audit_log(repo, audit_log) {
                    Ok(report) => {
                        let exit = report.exit_code();
                        let out = output::format_audit_verification_report(&report, &format);
                        println!("{out}");
                        std::process::exit(exit);
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        },
    }
}
