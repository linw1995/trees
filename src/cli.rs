use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "trees",
    version,
    about = "Manage coding workspaces composed of Git worktrees."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Create(CreateArgs),
    Codex(CodexArgs),
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[arg(value_name = "WORKSPACE_PATH")]
    pub workspace_path: PathBuf,

    #[arg(long = "repo", required = true, value_name = "REPOSITORY_PATH")]
    pub repositories: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub struct CodexArgs {
    #[arg(value_name = "WORKSPACE_PATH")]
    pub workspace_path: PathBuf,

    #[arg(long = "codex-bin", default_value = "codex", value_name = "PATH")]
    pub codex_bin: PathBuf,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn parses_multiple_repository_arguments() {
        let cli = Cli::try_parse_from([
            "trees",
            "create",
            "/tmp/workspace",
            "--repo",
            "/tmp/one",
            "--repo",
            "/tmp/two",
        ])
        .expect("create command should parse");

        let Command::Create(arguments) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(arguments.workspace_path, PathBuf::from("/tmp/workspace"));
        assert_eq!(
            arguments.repositories,
            [PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")]
        );
    }

    #[test]
    fn requires_at_least_one_repository() {
        assert!(Cli::try_parse_from(["trees", "create", "/tmp/workspace"]).is_err());
    }

    #[test]
    fn parses_codex_workspace_and_executable_override() {
        let cli = Cli::try_parse_from([
            "trees",
            "codex",
            "/tmp/workspace",
            "--codex-bin",
            "/opt/codex",
        ])
        .expect("codex command should parse");

        let Command::Codex(arguments) = cli.command else {
            panic!("expected codex command");
        };
        assert_eq!(arguments.workspace_path, PathBuf::from("/tmp/workspace"));
        assert_eq!(arguments.codex_bin, PathBuf::from("/opt/codex"));
    }

    #[test]
    fn requires_codex_workspace_path() {
        assert!(Cli::try_parse_from(["trees", "codex"]).is_err());
    }
}
