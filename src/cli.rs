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
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[arg(value_name = "WORKSPACE_PATH")]
    pub workspace_path: PathBuf,

    #[arg(long = "repo", required = true, value_name = "REPOSITORY_PATH")]
    pub repositories: Vec<PathBuf>,
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

        let Command::Create(arguments) = cli.command;
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
}
