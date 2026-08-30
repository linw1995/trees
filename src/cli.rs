use std::ffi::OsString;
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
    Checkin(CheckinArgs),
    Codex(CodexArgs),
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[arg(value_name = "WORKSPACE_PATH")]
    pub workspace_path: Option<PathBuf>,

    #[arg(long = "repo", required = true, value_name = "REPOSITORY_PATH")]
    pub repositories: Vec<PathBuf>,

    #[arg(long = "checkout-id", value_name = "CHECKOUT_ID")]
    pub checkout_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct CheckinArgs {
    #[arg(value_name = "WORKSPACE_PATH")]
    pub workspace_path: PathBuf,

    #[arg(long = "checkout-id", required = true, value_name = "CHECKOUT_ID")]
    pub checkout_id: String,
}

#[derive(Debug, Args)]
pub struct CodexArgs {
    #[arg(
        long = "codex-bin",
        default_value = "codex",
        value_name = "PATH",
        global = true
    )]
    pub codex_bin: PathBuf,

    #[command(subcommand)]
    pub subcommand: Option<CodexSubcommand>,

    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CODEX_ARG"
    )]
    pub codex_args: Vec<OsString>,
}

#[derive(Debug, Subcommand)]
pub enum CodexSubcommand {
    Resume(CodexResumeArgs),
}

#[derive(Debug, Args)]
pub struct CodexResumeArgs {
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CODEX_ARG"
    )]
    pub codex_args: Vec<OsString>,
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
        assert_eq!(
            arguments.workspace_path,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert_eq!(
            arguments.repositories,
            [PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")]
        );
        assert_eq!(arguments.checkout_id, None);
    }

    #[test]
    fn parses_automatic_create_without_a_workspace_path() {
        let cli = Cli::try_parse_from([
            "trees", "create", "--repo", "/tmp/one", "--repo", "/tmp/two",
        ])
        .expect("automatic create command should parse");

        let Command::Create(arguments) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(arguments.workspace_path, None);
        assert_eq!(
            arguments.repositories,
            [PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")]
        );
        assert_eq!(arguments.checkout_id, None);
    }

    #[test]
    fn parses_automatic_create_renewal() {
        let cli = Cli::try_parse_from([
            "trees",
            "create",
            "--repo",
            "/tmp/one",
            "--checkout-id",
            "checkout-id",
        ])
        .expect("automatic renewal command should parse");

        let Command::Create(arguments) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(arguments.workspace_path, None);
        assert_eq!(arguments.checkout_id.as_deref(), Some("checkout-id"));
    }

    #[test]
    fn parses_checkin_with_a_checkout_id() {
        let cli = Cli::try_parse_from([
            "trees",
            "checkin",
            "/tmp/workspace",
            "--checkout-id",
            "checkout-id",
        ])
        .expect("checkin command should parse");

        let Command::Checkin(arguments) = cli.command else {
            panic!("expected checkin command");
        };
        assert_eq!(arguments.workspace_path, PathBuf::from("/tmp/workspace"));
        assert_eq!(arguments.checkout_id, "checkout-id");
    }

    #[test]
    fn requires_at_least_one_repository() {
        assert!(Cli::try_parse_from(["trees", "create", "/tmp/workspace"]).is_err());
    }

    #[test]
    fn parses_codex_arguments_without_separator() {
        let cli = Cli::try_parse_from([
            "trees",
            "codex",
            "--codex-bin",
            "/opt/codex",
            "-C",
            "/tmp/workspace",
            "--model",
            "gpt-5.5",
            "--add-dir",
            "/tmp/extra",
        ])
        .expect("codex command should parse");

        let Command::Codex(arguments) = cli.command else {
            panic!("expected codex command");
        };
        assert!(arguments.subcommand.is_none());
        assert_eq!(
            arguments.codex_args,
            [
                OsString::from("-C"),
                OsString::from("/tmp/workspace"),
                OsString::from("--model"),
                OsString::from("gpt-5.5"),
                OsString::from("--add-dir"),
                OsString::from("/tmp/extra")
            ]
        );
        assert_eq!(arguments.codex_bin, PathBuf::from("/opt/codex"));
    }

    #[test]
    fn parses_codex_resume_arguments_without_separator() {
        let cli = Cli::try_parse_from([
            "trees",
            "codex",
            "resume",
            "-C",
            "/tmp/workspace",
            "--all",
            "--profile",
            "work",
        ])
        .expect("codex resume command should parse");

        let Command::Codex(arguments) = cli.command else {
            panic!("expected codex command");
        };
        let Some(CodexSubcommand::Resume(resume)) = arguments.subcommand else {
            panic!("expected resume subcommand");
        };
        assert_eq!(
            resume.codex_args,
            [
                OsString::from("-C"),
                OsString::from("/tmp/workspace"),
                OsString::from("--all"),
                OsString::from("--profile"),
                OsString::from("work")
            ]
        );
    }

    #[test]
    fn allows_codex_without_a_workspace_argument() {
        assert!(Cli::try_parse_from(["trees", "codex"]).is_ok());
    }
}
