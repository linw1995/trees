use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = trees::cli::Cli::parse();
    let result = match cli.command {
        trees::cli::Command::Create(arguments) => {
            trees::workspace::create(trees::workspace::CreateRequest {
                workspace_path: arguments.workspace_path,
                repositories: arguments.repositories,
            })
        }
        trees::cli::Command::Codex(_) => {
            eprintln!("Error: codex command is not implemented yet");
            return ExitCode::FAILURE;
        }
    };

    match result {
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
