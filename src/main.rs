use std::process::{ExitCode, ExitStatus};

use clap::Parser;

fn main() -> ExitCode {
    run(trees::cli::Cli::parse())
}

fn run(cli: trees::cli::Cli) -> ExitCode {
    match cli.command {
        trees::cli::Command::Create(arguments) => run_create(arguments),
        trees::cli::Command::Codex(arguments) => run_codex(arguments),
    }
}

fn run_create(arguments: trees::cli::CreateArgs) -> ExitCode {
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

fn run_codex(arguments: trees::cli::CodexArgs) -> ExitCode {
    if arguments.subcommand.is_some() || !arguments.codex_args.is_empty() {
        eprintln!("Error: native Codex handoff is not implemented yet");
        return ExitCode::FAILURE;
    }

    match trees::codex::launch::launch(trees::codex::launch::LaunchRequest {
        workspace_path: std::path::PathBuf::from("."),
        codex_bin: arguments.codex_bin,
    }) {
        Ok(status) => exit_code(status),
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn reports_create_validation_errors() {
        let cli = trees::cli::Cli::try_parse_from([
            "trees",
            "create",
            "/tmp/trees-coverage-workspace",
            "--repo",
            "/tmp/trees-coverage-missing-repository",
        ])
        .expect("create command should parse");

        assert_eq!(run(cli), ExitCode::FAILURE);
    }
}
