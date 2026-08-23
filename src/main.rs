use std::process::ExitCode;

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
            match trees::codex::launch::prepare_launch(trees::codex::launch::LaunchRequest {
                workspace_path: arguments.workspace_path,
                codex_bin: arguments.codex_bin,
            }) {
                Ok(result) => {
                    println!("Prepared Codex project: {}", result.project_id);
                    println!("Prepared Codex thread: {}", result.thread_id);
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
