use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "trees",
    version,
    long_version = concat!(
        env!("CARGO_PKG_VERSION"),
        "\ncommit: ", env!("GIT_COMMIT_SHA"),
        "\ndirty: ", env!("GIT_DIRTY"),
        "\nbuild time (UTC): ", env!("BUILT_TIME_UTC")
    ),
    about = "Manage coding workspaces composed of Git worktrees."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Create(CreateArgs),
    Release(ReleaseArgs),
    Config(ConfigArgs),
    Gc(GcArgs),
    #[command(about = "Remove a workspace or an unused source repository record by ID")]
    Remove(RemoveArgs),
    #[command(about = "Inspect persisted pools, workspaces, or source repositories")]
    Status(StatusArgs),
    Open(OpenArgs),
    Codex(CodexArgs),
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[arg(value_name = "WORKSPACE_PATH")]
    pub workspace_path: Option<PathBuf>,

    #[arg(
        long = "repo",
        required = true,
        value_name = "PATH|URL|NAME",
        help = "Use a local path, remote URL, or registered source directory name"
    )]
    pub repositories: Vec<PathBuf>,

    #[arg(long, help = "Print the create result as JSON")]
    pub json: bool,

    #[arg(long, help = "Use local HEAD without fetching remotes")]
    pub offline: bool,

    #[arg(
        long,
        value_name = "PROGRAM",
        require_equals = true,
        conflicts_with = "json",
        help = "Open a program in the workspace, defaulting to $SHELL"
    )]
    pub open: Option<Option<OsString>>,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("target")
        .multiple(false)
        .args(["workspace_dir", "claim_id"])
))]
pub struct ReleaseArgs {
    #[arg(value_name = "WORKSPACE_DIR")]
    pub workspace_dir: Option<PathBuf>,

    #[arg(long = "claim-id", value_name = "CLAIM_ID")]
    pub claim_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Set(ConfigSetArgs),
}

#[derive(Debug, Args)]
pub struct ConfigSetArgs {
    #[arg(value_enum, value_name = "SETTING")]
    pub setting: ConfigSetting,

    #[arg(value_name = "VALUE")]
    pub value: PathBuf,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum ConfigSetting {
    WorkspacesDir,
    OriginsDir,
}

#[derive(Debug, Args)]
pub struct GcArgs {
    #[arg(long = "older-than", value_name = "DURATION")]
    pub older_than: crate::gc::GcDuration,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub yes: bool,

    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    #[arg(value_name = "WORKSPACE_OR_REPO_ID")]
    pub workspace_id: crate::domain::WorkspaceId,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub yes: bool,

    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(long, value_enum, default_value_t = StatusView::Pools)]
    pub view: StatusView,

    #[arg(long, help = "Include reclaimed records in the workspace view")]
    pub all: bool,

    #[arg(long, help = "Print the workspace status snapshot as JSON")]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum StatusView {
    Pools,
    Workspaces,
    Repos,
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: crate::domain::WorkspaceId,

    #[arg(long, value_name = "PROGRAM", require_equals = true)]
    pub program: Option<OsString>,
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
        assert!(!arguments.json);
        assert!(!arguments.offline);
        assert!(arguments.open.is_none());
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
        assert!(!arguments.json);
        assert!(!arguments.offline);
        assert!(arguments.open.is_none());
    }

    #[test]
    fn parses_create_json_output_flag() {
        let cli = Cli::try_parse_from(["trees", "create", "--repo", "/tmp/one", "--json"])
            .expect("create command should parse");

        let Command::Create(arguments) = cli.command else {
            panic!("expected create command");
        };
        assert!(arguments.json);
        assert!(arguments.open.is_none());
    }

    #[test]
    fn parses_create_offline_flag() {
        let cli = Cli::try_parse_from(["trees", "create", "--repo", "/tmp/one", "--offline"])
            .expect("offline create command should parse");

        let Command::Create(arguments) = cli.command else {
            panic!("expected create command");
        };
        assert!(arguments.offline);
    }

    #[test]
    fn parses_create_open_with_the_default_program() {
        let cli = Cli::try_parse_from(["trees", "create", "--repo", "/tmp/one", "--open"])
            .expect("create command should parse");

        let Command::Create(arguments) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(arguments.open, Some(None));
    }

    #[test]
    fn parses_create_open_with_an_explicit_program() {
        let cli = Cli::try_parse_from([
            "trees",
            "create",
            "--repo",
            "/tmp/one",
            "--open=/usr/bin/env",
        ])
        .expect("create command should parse");

        let Command::Create(arguments) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(arguments.open, Some(Some(OsString::from("/usr/bin/env"))));
    }

    #[test]
    fn parses_an_empty_explicit_open_program_for_validation() {
        let cli = Cli::try_parse_from(["trees", "create", "--repo", "/tmp/one", "--open="])
            .expect("create command should parse before program validation");

        let Command::Create(arguments) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(arguments.open, Some(Some(OsString::new())));
    }

    #[test]
    fn rejects_combined_create_open_and_json() {
        assert!(
            Cli::try_parse_from(["trees", "create", "--repo", "/tmp/one", "--open", "--json"])
                .is_err()
        );
    }

    #[test]
    fn parses_release_with_a_relative_workspace_directory() {
        let cli = Cli::try_parse_from(["trees", "release", "relative/workspace"])
            .expect("release command should parse");

        let Command::Release(arguments) = cli.command else {
            panic!("expected release command");
        };
        assert_eq!(
            arguments.workspace_dir,
            Some(PathBuf::from("relative/workspace"))
        );
        assert_eq!(arguments.claim_id, None);
    }

    #[test]
    fn parses_release_without_a_target() {
        let cli = Cli::try_parse_from(["trees", "release"]).expect("release command should parse");

        let Command::Release(arguments) = cli.command else {
            panic!("expected release command");
        };
        assert_eq!(arguments.workspace_dir, None);
        assert_eq!(arguments.claim_id, None);
    }

    #[test]
    fn parses_release_with_a_claim_id() {
        let cli = Cli::try_parse_from(["trees", "release", "--claim-id", "claim-id"])
            .expect("release command should parse");

        let Command::Release(arguments) = cli.command else {
            panic!("expected release command");
        };
        assert_eq!(arguments.workspace_dir, None);
        assert_eq!(arguments.claim_id.as_deref(), Some("claim-id"));
    }

    #[test]
    fn rejects_combined_release_targets_and_the_removed_cwd_option() {
        for arguments in [
            vec![
                "trees",
                "release",
                "/tmp/workspace",
                "--claim-id",
                "claim-id",
            ],
            vec!["trees", "release", "--cwd"],
        ] {
            assert!(Cli::try_parse_from(arguments).is_err());
        }
    }

    #[test]
    fn rejects_the_legacy_checkout_id_option() {
        assert!(Cli::try_parse_from(["trees", "release", "--checkout-id", "claim-id",]).is_err());
    }

    #[test]
    fn parses_workspace_directory_configuration() {
        let cli = Cli::try_parse_from([
            "trees",
            "config",
            "set",
            "workspaces-dir",
            "relative-workspaces",
        ])
        .expect("configuration command should parse");

        let Command::Config(arguments) = cli.command else {
            panic!("expected config command");
        };
        let ConfigCommand::Set(arguments) = arguments.command;
        assert!(matches!(arguments.setting, ConfigSetting::WorkspacesDir));
        assert_eq!(arguments.value, PathBuf::from("relative-workspaces"));
    }

    #[test]
    fn parses_gc_threshold_and_confirmation_flags() {
        let cli = Cli::try_parse_from([
            "trees",
            "gc",
            "--older-than",
            "30d",
            "--dry-run",
            "--yes",
            "--force",
        ])
        .expect("GC command should parse");

        let Command::Gc(arguments) = cli.command else {
            panic!("expected GC command");
        };
        assert_eq!(arguments.older_than.seconds(), 30 * 24 * 60 * 60);
        assert!(arguments.dry_run);
        assert!(arguments.yes);
        assert!(arguments.force);
    }

    #[test]
    fn parses_workspace_remove_and_safety_flags() {
        let workspace_id = crate::domain::WorkspaceId::new();
        let cli = Cli::try_parse_from([
            "trees".to_owned(),
            "remove".to_owned(),
            workspace_id.to_string(),
            "--dry-run".to_owned(),
            "--yes".to_owned(),
            "--force".to_owned(),
        ])
        .expect("remove command should parse");

        let Command::Remove(arguments) = cli.command else {
            panic!("expected remove command");
        };
        assert_eq!(arguments.workspace_id, workspace_id);
        assert!(arguments.dry_run);
        assert!(arguments.yes);
        assert!(arguments.force);
    }

    #[test]
    fn rejects_an_invalid_workspace_remove_identifier() {
        assert!(Cli::try_parse_from(["trees", "remove", "invalid"]).is_err());
    }

    #[test]
    fn parses_status_output_and_reclaimed_filters() {
        let cli =
            Cli::try_parse_from(["trees", "status", "--view", "workspaces", "--all", "--json"])
                .expect("status command should parse");

        let Command::Status(arguments) = cli.command else {
            panic!("expected status command");
        };
        assert_eq!(arguments.view, StatusView::Workspaces);
        assert!(arguments.all);
        assert!(arguments.json);
    }

    #[test]
    fn defaults_status_to_the_pool_view() {
        let cli = Cli::try_parse_from(["trees", "status"]).expect("status command should parse");

        let Command::Status(arguments) = cli.command else {
            panic!("expected status command");
        };
        assert_eq!(arguments.view, StatusView::Pools);
        assert!(!arguments.all);
    }

    #[test]
    fn parses_workspace_open_with_an_explicit_program() {
        let workspace_id = crate::domain::WorkspaceId::new();
        let cli = Cli::try_parse_from([
            "trees".to_owned(),
            "open".to_owned(),
            workspace_id.to_string(),
            "--program=/usr/bin/env".to_owned(),
        ])
        .expect("open command should parse");

        let Command::Open(arguments) = cli.command else {
            panic!("expected open command");
        };
        assert_eq!(arguments.workspace_id, workspace_id);
        assert_eq!(arguments.program, Some(OsString::from("/usr/bin/env")));
    }

    #[test]
    fn rejects_an_invalid_workspace_open_identifier() {
        assert!(Cli::try_parse_from(["trees", "open", "invalid"]).is_err());
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
