use std::process::{ExitCode, ExitStatus};

use clap::Parser;

fn main() -> ExitCode {
    let cli = trees::cli::Cli::parse();
    match cli.command {
        trees::cli::Command::Create(arguments) => {
            match trees::workspace::create(trees::workspace::CreateRequest {
                workspace_path: arguments.workspace_path,
                repositories: arguments.repositories,
            }) {
                Ok(result) => {
                    println!("Created workspace: {}", result.workspace_path);
                    for path in result.worktree_paths {
                        println!("Attached worktree: {}", path.display());
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        trees::cli::Command::Codex(arguments) => {
            match trees::codex::launch::launch(trees::codex::launch::LaunchRequest {
                workspace_path: arguments.workspace_path,
                codex_bin: arguments.codex_bin,
            }) {
                Ok(status) => exit_code(status),
                Err(error) => {
                    eprintln!("Error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn exit_code(status: ExitStatus) -> ExitCode {
    if status.success() {
        return ExitCode::SUCCESS;
    }

    match status.code() {
        Some(code) if (0..=u8::MAX as i32).contains(&code) => ExitCode::from(code as u8),
        _ => ExitCode::FAILURE,
    }
}
