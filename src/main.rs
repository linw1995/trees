use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};

use clap::Parser;
use snafu::{ResultExt, Snafu};

fn main() -> ExitCode {
    match run(trees::cli::Cli::parse()) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: trees::cli::Cli) -> Result<ExitCode, CliError> {
    match cli.command {
        trees::cli::Command::Create(arguments) => run_create(arguments),
        trees::cli::Command::Release(arguments) => run_release(arguments),
        trees::cli::Command::Config(arguments) => run_config(arguments),
        trees::cli::Command::Gc(arguments) => run_gc(arguments),
        trees::cli::Command::Remove(arguments) => run_remove(arguments),
        trees::cli::Command::Status(arguments) => run_status(arguments),
        trees::cli::Command::Open(arguments) => run_open(arguments),
        trees::cli::Command::Codex(arguments) => run_codex(arguments),
    }
}

fn run_create(arguments: trees::cli::CreateArgs) -> Result<ExitCode, CliError> {
    let trees::cli::CreateArgs {
        workspace_path,
        repositories,
        json,
        offline,
        open,
    } = arguments;
    let open = resolve_open_program(open)?;
    if let Some(path) = &workspace_path {
        trees::validation::validate_new_workspace_target(path)?;
    }
    let expected = trees::origin::resolve::resolve(&repositories, offline)?;
    let repositories = expected
        .iter()
        .map(|info| info.root.as_path().to_owned())
        .collect();
    match workspace_path {
        Some(workspace_path) => {
            let result = trees::workspace::create_resolved(
                trees::workspace::CreateRequest {
                    workspace_path,
                    repositories,
                    offline,
                },
                &expected,
            )?;
            if let Some(program) = open.as_deref() {
                return open_workspace(program, result.workspace_path.as_path());
            }
            if json {
                return print_json(&result);
            }
            println!("Created workspace: {}", result.workspace_path);
            for path in result.worktree_paths {
                println!("Attached worktree: {}", path.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        None => run_automatic_create(repositories, json, offline, open.as_deref(), &expected),
    }
}

fn resolve_open_program(open: Option<Option<OsString>>) -> Result<Option<OsString>, CliError> {
    match open {
        None => Ok(None),
        Some(program) => resolve_required_program(program, "--open").map(Some),
    }
}

fn resolve_required_program(
    program: Option<OsString>,
    option_name: &'static str,
) -> Result<OsString, CliError> {
    match program {
        Some(program) if !program.is_empty() => Ok(program),
        Some(_) => EmptyProgramSnafu { option_name }.fail(),
        None => std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .ok_or_else(|| CliError::ShellUnavailable { option_name }),
    }
}

fn run_automatic_create(
    repositories: Vec<std::path::PathBuf>,
    json: bool,
    offline: bool,
    open: Option<&OsStr>,
    expected: &[trees::git::RepositoryInfo],
) -> Result<ExitCode, CliError> {
    let plan = trees::workspace::prepare_automatic_resolved(
        &trees::workspace::AutomaticCreateRequest {
            repositories,
            offline,
        },
        expected,
    )?;
    let mut connection = trees::database::open_default()?;
    run_automatic_allocation(&mut connection, &plan, json, open)
}

fn run_automatic_allocation(
    connection: &mut diesel::sqlite::SqliteConnection,
    plan: &trees::workspace::AutomaticAllocationPlan,
    json: bool,
    open: Option<&OsStr>,
) -> Result<ExitCode, CliError> {
    let result = trees::workspace::allocate_automatic_workspace(connection, plan)?;
    if let Some(program) = open {
        return open_workspace(program, result.workspace_path.as_path());
    }
    if json {
        print_json(&result)
    } else {
        print_automatic_claim_result(&result);
        Ok(ExitCode::SUCCESS)
    }
}

#[cfg(unix)]
fn open_workspace(program: &OsStr, workspace_path: &Path) -> Result<ExitCode, CliError> {
    use std::os::unix::process::CommandExt;

    let error = std::process::Command::new(program)
        .current_dir(workspace_path)
        .exec();
    Err(CliError::OpenWorkspace {
        program: PathBuf::from(program),
        workspace_path: workspace_path.to_owned(),
        source: error,
    })
}

fn run_open(arguments: trees::cli::OpenArgs) -> Result<ExitCode, CliError> {
    let program = resolve_required_program(arguments.program, "--program")?;
    let mut connection = trees::database::open_read_only()?;
    let workspace_path =
        trees::workspace_open::resolve_target(&mut connection, &arguments.workspace_id)?;
    drop(connection);
    open_workspace(&program, workspace_path.as_path())
}

#[cfg(not(unix))]
fn open_workspace(program: &OsStr, workspace_path: &Path) -> Result<ExitCode, CliError> {
    match std::process::Command::new(program)
        .current_dir(workspace_path)
        .status()
    {
        Ok(status) => Ok(exit_code(status)),
        Err(source) => Err(CliError::OpenWorkspace {
            program: PathBuf::from(program),
            workspace_path: workspace_path.to_owned(),
            source,
        }),
    }
}

fn run_release(arguments: trees::cli::ReleaseArgs) -> Result<ExitCode, CliError> {
    let result = release_automatic(&arguments)?;
    println!("{}", release_output(&result));
    Ok(ExitCode::SUCCESS)
}

fn release_output(result: &trees::workspace::ReleaseResult) -> String {
    format!(
        "workspace_id={}\nworkspace_path={}\nclaim_id={}\nreleased_at={}",
        result.workspace_id, result.workspace_path, result.claim_id, result.released_at,
    )
}

fn release_automatic(
    arguments: &trees::cli::ReleaseArgs,
) -> Result<trees::workspace::ReleaseResult, CliError> {
    let target = release_target(arguments)?;
    let mut connection = trees::database::open_default()?;
    Ok(trees::workspace::release_automatic_workspace_by_target(
        &mut connection,
        target,
    )?)
}

fn release_target(
    arguments: &trees::cli::ReleaseArgs,
) -> Result<trees::workspace::ReleaseTarget, CliError> {
    match arguments.workspace_dir.as_ref() {
        Some(path) => workspace_path_release_target(path),
        None => match arguments.claim_id.as_deref() {
            Some(claim_id) => claim_release_target(claim_id),
            None => current_directory_release_target(),
        },
    }
}

fn workspace_path_release_target(
    path: &std::path::Path,
) -> Result<trees::workspace::ReleaseTarget, CliError> {
    Ok(trees::workspace::ReleaseTarget::WorkspacePath(
        trees::validation::resolve_workspace_path(path)?,
    ))
}

fn current_directory_release_target() -> Result<trees::workspace::ReleaseTarget, CliError> {
    Ok(trees::workspace::ReleaseTarget::CurrentDirectory(
        trees::domain::CanonicalPath::resolve(".")?,
    ))
}

fn claim_release_target(claim_id: &str) -> Result<trees::workspace::ReleaseTarget, CliError> {
    Ok(trees::workspace::ReleaseTarget::ClaimId(
        claim_id
            .parse::<trees::domain::ClaimId>()
            .context(ClaimIdSnafu)?,
    ))
}

fn run_config(arguments: trees::cli::ConfigArgs) -> Result<ExitCode, CliError> {
    match arguments.command {
        trees::cli::ConfigCommand::Set(arguments) => match arguments.setting {
            trees::cli::ConfigSetting::OriginsDir => {
                let path = trees::config::set_origins_directory(&arguments.value)?;
                println!("origins_dir={}", path.display());
                Ok(ExitCode::SUCCESS)
            }
            trees::cli::ConfigSetting::WorkspacesDir => {
                let path = trees::config::set_workspaces_directory(&arguments.value)?;
                println!("workspaces_dir={}", path.display());
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

fn run_gc(arguments: trees::cli::GcArgs) -> Result<ExitCode, CliError> {
    run_gc_command(&arguments)
}

fn run_gc_command(arguments: &trees::cli::GcArgs) -> Result<ExitCode, CliError> {
    let scan = load_gc_scan(arguments)?;
    print_gc_scan(&scan, arguments.force);
    run_gc_after_scan(arguments, &scan)
}

fn run_gc_after_scan(
    arguments: &trees::cli::GcArgs,
    scan: &trees::gc::GcScan,
) -> Result<ExitCode, CliError> {
    if arguments.dry_run {
        return Ok(ExitCode::SUCCESS);
    }
    match confirm_gc(
        arguments.force,
        arguments.yes,
        scan.execution_candidate_count(arguments.force),
    )? {
        GcConfirmation::Proceed => execute_gc(arguments),
        GcConfirmation::Cancelled => Ok(ExitCode::SUCCESS),
    }
}

fn load_gc_scan(arguments: &trees::cli::GcArgs) -> Result<trees::gc::GcScan, CliError> {
    let mut connection = trees::database::open_read_only()?;
    Ok(trees::gc::scan_with_force(
        &mut connection,
        arguments.older_than,
        arguments.force,
    )?)
}

fn print_gc_scan(scan: &trees::gc::GcScan, force: bool) {
    println!("cutoff={}", scan.cutoff);
    println!("automatic={}", scan.counts.automatic);
    println!("unclaimed={}", scan.counts.unclaimed);
    println!("claimed={}", scan.counts.claimed);
    println!("age_eligible={}", scan.counts.age_eligible);
    println!("safe_to_reclaim={}", scan.counts.safe_to_reclaim);
    println!("candidates={}", scan.execution_candidate_count(force));
    for candidate in &scan.candidates {
        println!(
            "candidate={} reason={}",
            candidate.workspace.canonical_path,
            candidate.reason(force)
        );
    }
}

enum GcConfirmation {
    Proceed,
    Cancelled,
}

fn confirm_gc(force: bool, yes: bool, candidate_count: usize) -> Result<GcConfirmation, CliError> {
    if force {
        eprintln!("Warning: --force may remove dirty worktrees and unexpected workspace content.");
    } else if yes || candidate_count == 0 {
        return Ok(GcConfirmation::Proceed);
    } else {
        return confirm_gc_interactively(candidate_count);
    }
    Ok(GcConfirmation::Proceed)
}

fn confirm_gc_interactively(candidate_count: usize) -> Result<GcConfirmation, CliError> {
    if !io::stdin().is_terminal() {
        return InteractiveConfirmationUnavailableSnafu.fail();
    }
    print!("Reclaim {candidate_count} workspaces? [y/N] ");
    io::stdout().flush().context(FlushConfirmationSnafu)?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context(ReadConfirmationSnafu)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        println!("cancelled=true");
        return Ok(GcConfirmation::Cancelled);
    }
    Ok(GcConfirmation::Proceed)
}

fn execute_gc(arguments: &trees::cli::GcArgs) -> Result<ExitCode, CliError> {
    let report = execute_gc_report(arguments)?;
    println!("unclaimed={}", report.scan.counts.unclaimed);
    println!("claimed={}", report.scan.counts.claimed);
    println!("reclaimed={}", report.reclaimed.len());
    println!("skipped={}", report.skipped.len());
    println!("failed={}", report.failed.len());
    if !report.failed.is_empty() {
        for failure in report.failed {
            eprintln!("GC failed: {}: {}", failure.workspace_path, failure.error);
        }
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

fn execute_gc_report(
    arguments: &trees::cli::GcArgs,
) -> Result<trees::gc::GcExecutionReport, CliError> {
    let mut connection = trees::database::open_default()?;
    Ok(trees::gc::execute(
        &mut connection,
        arguments.older_than,
        arguments.force,
    )?)
}

fn run_remove(arguments: trees::cli::RemoveArgs) -> Result<ExitCode, CliError> {
    run_remove_command(&arguments)
}

fn run_remove_command(arguments: &trees::cli::RemoveArgs) -> Result<ExitCode, CliError> {
    let preflight = load_removal_preflight(arguments)?;
    println!("workspace_id={}", preflight.workspace.id);
    println!("workspace_path={}", preflight.workspace.canonical_path);
    println!("preflight_reason={}", preflight.reason);
    if arguments.dry_run {
        return Ok(ExitCode::SUCCESS);
    }
    if !preflight.can_execute() {
        return Ok(ExitCode::FAILURE);
    }
    match confirm_removal(arguments.force, arguments.yes)? {
        GcConfirmation::Proceed => execute_removal(arguments),
        GcConfirmation::Cancelled => Ok(ExitCode::SUCCESS),
    }
}

fn load_removal_preflight(
    arguments: &trees::cli::RemoveArgs,
) -> Result<trees::gc::RemovalPreflight, CliError> {
    let mut connection = trees::database::open_read_only()?;
    Ok(trees::gc::scan_removal(
        &mut connection,
        &arguments.workspace_id,
        arguments.force,
    )?)
}

fn confirm_removal(force: bool, yes: bool) -> Result<GcConfirmation, CliError> {
    if force {
        eprintln!("Warning: --force may remove dirty worktrees and unexpected workspace content.");
        return Ok(GcConfirmation::Proceed);
    }
    if yes {
        return Ok(GcConfirmation::Proceed);
    }
    if !io::stdin().is_terminal() {
        return InteractiveConfirmationUnavailableSnafu.fail();
    }
    print!("Remove this workspace? [y/N] ");
    io::stdout().flush().context(FlushConfirmationSnafu)?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context(ReadConfirmationSnafu)?;
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(GcConfirmation::Proceed)
    } else {
        println!("cancelled=true");
        Ok(GcConfirmation::Cancelled)
    }
}

fn execute_removal(arguments: &trees::cli::RemoveArgs) -> Result<ExitCode, CliError> {
    let mut connection = trees::database::open_default()?;
    let report =
        trees::gc::remove_workspace(&mut connection, &arguments.workspace_id, arguments.force)?;
    println!("removed={}", report.removed);
    if !report.removed {
        println!("reason={}", report.reason);
    }
    if let Some(error) = report.error {
        eprintln!("Remove failed: {}: {error}", report.workspace_path);
    }
    Ok(if report.removed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn run_status(arguments: trees::cli::StatusArgs) -> Result<ExitCode, CliError> {
    if arguments.all && arguments.view == trees::cli::StatusView::Pools {
        return InvalidStatusArgumentsSnafu.fail();
    }
    match arguments.view {
        trees::cli::StatusView::Pools => run_pool_status(arguments.json),
        trees::cli::StatusView::Repos => run_repo_status(arguments.all, arguments.json),
        trees::cli::StatusView::Workspaces => run_workspace_status(arguments.all, arguments.json),
    }
}

fn run_repo_status(all: bool, json: bool) -> Result<ExitCode, CliError> {
    let snapshot = match trees::database::open_read_only() {
        Ok(mut connection) => {
            trees::status::repos::load(&mut connection, all).context(RepoStatusSnafu)?
        }
        Err(trees::database::DatabaseError::ReadOnlyDatabaseMissing { .. }) => {
            trees::status::repos::RepoSnapshot::empty()
        }
        Err(source) => return Err(source.into()),
    };
    if json {
        print_json(&snapshot)
    } else {
        println!("{}", trees::status::repos::render(&snapshot));
        Ok(ExitCode::SUCCESS)
    }
}

fn run_pool_status(json: bool) -> Result<ExitCode, CliError> {
    let snapshot = load_pool_status_snapshot()?;
    if json {
        print_json(&snapshot)
    } else {
        println!(
            "{}",
            trees::status::render_pools_human(&snapshot, status_color_enabled())
        );
        Ok(ExitCode::SUCCESS)
    }
}

fn load_pool_status_snapshot() -> Result<trees::status::PoolStatusSnapshot, CliError> {
    let mut connection = match trees::database::open_read_only() {
        Ok(connection) => connection,
        Err(trees::database::DatabaseError::ReadOnlyDatabaseMissing { .. }) => {
            return Ok(trees::status::PoolStatusSnapshot::empty());
        }
        Err(source) => return Err(source.into()),
    };
    trees::status::load_pool_snapshot(&mut connection).context(PoolStatusSnafu)
}

fn run_workspace_status(include_reclaimed: bool, json: bool) -> Result<ExitCode, CliError> {
    let snapshot = load_workspace_status_snapshot(include_reclaimed)?;
    if json {
        print_json(&snapshot)
    } else {
        println!(
            "{}",
            trees::status::render_workspaces_human(&snapshot, status_color_enabled())
        );
        Ok(ExitCode::SUCCESS)
    }
}

fn status_color_enabled() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn load_workspace_status_snapshot(
    include_reclaimed: bool,
) -> Result<trees::status::StatusSnapshot, CliError> {
    let mut connection = match trees::database::open_read_only() {
        Ok(connection) => connection,
        Err(trees::database::DatabaseError::ReadOnlyDatabaseMissing { .. }) => {
            return Ok(trees::status::StatusSnapshot::empty());
        }
        Err(source) => return Err(source.into()),
    };
    trees::status::load_snapshot(&mut connection, include_reclaimed).context(WorkspaceStatusSnafu)
}

fn print_automatic_claim_result(result: &trees::workspace::AutomaticClaimResult) {
    println!("{}", automatic_claim_shell_output(result));
}

fn automatic_claim_shell_output(result: &trees::workspace::AutomaticClaimResult) -> String {
    format!(
        "WORKSPACE_PATH={}\nPOOL_ID={}\nCLAIM_ID={}",
        bash_quote(&result.workspace_path.to_string()),
        bash_quote(&result.pool_id.to_string()),
        bash_quote(&result.claim_id.to_string()),
    )
}

fn bash_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<ExitCode, CliError> {
    let output = serde_json::to_string(value).context(SerializeJsonSnafu)?;
    println!("{output}");
    Ok(ExitCode::SUCCESS)
}

fn run_codex(arguments: trees::cli::CodexArgs) -> Result<ExitCode, CliError> {
    let trees::cli::CodexArgs {
        codex_bin,
        subcommand,
        codex_args,
    } = arguments;

    let native_args = match &subcommand {
        None => &codex_args,
        Some(trees::cli::CodexSubcommand::Resume(resume)) => &resume.codex_args,
    };
    let workspace_path = trees::codex::args::workspace_path_from_codex_args(native_args)?;

    let result = match subcommand {
        None => trees::codex::launch::launch(trees::codex::launch::LaunchRequest {
            workspace_path,
            codex_bin,
            codex_args,
        }),
        Some(trees::cli::CodexSubcommand::Resume(resume)) => {
            trees::codex::launch::resume(trees::codex::launch::ResumeRequest {
                workspace_path,
                codex_bin,
                codex_args: resume.codex_args,
            })
        }
    };

    Ok(exit_code(result?))
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

#[derive(Debug, Snafu)]
enum CliError {
    #[snafu(transparent)]
    OriginInput {
        source: trees::origin::resolve::ResolveError,
    },
    #[snafu(display("{option_name} program must not be empty"))]
    EmptyProgram { option_name: &'static str },
    #[snafu(display("$SHELL is unset or empty; use {option_name}=<PROGRAM>"))]
    ShellUnavailable { option_name: &'static str },
    #[snafu(display(
        "failed to open {} in workspace {}: {source}",
        program.display(),
        workspace_path.display()
    ))]
    OpenWorkspace {
        program: PathBuf,
        workspace_path: PathBuf,
        source: io::Error,
    },
    #[snafu(transparent)]
    Database {
        source: trees::database::DatabaseError,
    },
    #[snafu(transparent)]
    Workspace {
        source: trees::workspace::WorkspaceError,
    },
    #[snafu(transparent)]
    Validation {
        source: trees::validation::ValidationError,
    },
    #[snafu(transparent)]
    CanonicalPath {
        source: trees::domain::CanonicalPathError,
    },
    #[snafu(display("invalid claim ID: {source}"))]
    ClaimId {
        source: trees::domain::IdentifierError,
    },
    #[snafu(transparent)]
    Config { source: trees::config::ConfigError },
    #[snafu(transparent)]
    WorkspaceOpen {
        source: trees::workspace_open::WorkspaceOpenError,
    },
    #[snafu(transparent)]
    Gc { source: trees::gc::GcError },
    #[snafu(display("interactive confirmation is unavailable; use --dry-run, --yes, or --force"))]
    InteractiveConfirmationUnavailable,
    #[snafu(display("failed to flush confirmation prompt: {source}"))]
    FlushConfirmation { source: io::Error },
    #[snafu(display("failed to read confirmation: {source}"))]
    ReadConfirmation { source: io::Error },
    #[snafu(display("--all requires --view workspaces or --view repos"))]
    InvalidStatusArguments,
    #[snafu(display("failed to load repository status; if the database schema is outdated, run create to upgrade it: {source}"))]
    RepoStatus { source: diesel::result::Error },
    #[snafu(display("failed to load workspace pool status: {source}"))]
    PoolStatus { source: diesel::result::Error },
    #[snafu(display("failed to load workspace status: {source}"))]
    WorkspaceStatus { source: diesel::result::Error },
    #[snafu(display("failed to serialize JSON output: {source}"))]
    SerializeJson { source: serde_json::Error },
    #[snafu(transparent)]
    CodexArguments {
        source: trees::codex::args::CodexArgumentError,
    },
    #[snafu(transparent)]
    CodexLaunch {
        source: trees::codex::launch::CodexLaunchError,
    },
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use clap::Parser;

    use super::*;

    static SHELL_ENV_LOCK: Mutex<()> = Mutex::new(());

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

        assert!(run(cli).is_err());
    }

    #[test]
    fn resolves_the_default_open_program_from_the_shell_environment() {
        let _guard = SHELL_ENV_LOCK.lock().expect("environment lock should work");
        let previous = std::env::var_os("SHELL");
        std::env::set_var("SHELL", "/bin/test-shell");

        let resolved = resolve_open_program(Some(None));

        match previous {
            Some(value) => std::env::set_var("SHELL", value),
            None => std::env::remove_var("SHELL"),
        }
        assert_eq!(
            resolved.expect("default open program should resolve"),
            Some(OsString::from("/bin/test-shell"))
        );
    }

    #[test]
    fn rejects_a_missing_default_open_program() {
        let _guard = SHELL_ENV_LOCK.lock().expect("environment lock should work");
        let previous = std::env::var_os("SHELL");
        std::env::remove_var("SHELL");

        let resolved = resolve_open_program(Some(None));

        if let Some(value) = previous {
            std::env::set_var("SHELL", value);
        }
        assert_eq!(
            resolved.expect_err("unset shell should fail").to_string(),
            "$SHELL is unset or empty; use --open=<PROGRAM>"
        );
    }

    #[test]
    fn rejects_an_empty_explicit_open_program() {
        assert_eq!(
            resolve_open_program(Some(Some(OsString::new())))
                .expect_err("empty program should fail")
                .to_string(),
            "--open program must not be empty"
        );
    }

    #[test]
    fn quotes_shell_output_values_for_bash() {
        assert_eq!(
            bash_quote("/tmp/workspace with space"),
            "'/tmp/workspace with space'"
        );
        assert_eq!(bash_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn formats_automatic_claim_as_bash_assignments() {
        let result = trees::workspace::AutomaticClaimResult {
            workspace_path: trees::domain::CanonicalPath::from_absolute("/tmp/workspace")
                .expect("workspace path should be absolute"),
            pool_id: trees::domain::PoolId::new(),
            claim_id: trees::domain::ClaimId::new(),
        };

        let output = automatic_claim_shell_output(&result);

        assert!(output.starts_with("WORKSPACE_PATH='/tmp/workspace'\nPOOL_ID='"));
        assert!(output.contains("\nCLAIM_ID='"));
    }

    #[test]
    fn serializes_automatic_claim_as_json() {
        let result = trees::workspace::AutomaticClaimResult {
            workspace_path: trees::domain::CanonicalPath::from_absolute("/tmp/workspace")
                .expect("workspace path should be absolute"),
            pool_id: trees::domain::PoolId::new(),
            claim_id: trees::domain::ClaimId::new(),
        };
        let output = serde_json::to_string(&result).expect("claim should serialize as JSON");
        let value: serde_json::Value =
            serde_json::from_str(&output).expect("claim JSON should be valid");

        assert_eq!(value["workspace_path"], "/tmp/workspace");
        let pool_id = value["pool_id"]
            .as_str()
            .expect("pool ID should be a string");
        assert!(pool_id.parse::<trees::domain::PoolId>().is_ok());
        assert!(value["claim_id"].is_string());
    }

    #[test]
    fn resolves_each_release_target() {
        let path_arguments = trees::cli::ReleaseArgs {
            workspace_dir: Some(std::path::PathBuf::from(".")),
            claim_id: None,
        };
        let path = match release_target(&path_arguments) {
            Ok(trees::workspace::ReleaseTarget::WorkspacePath(path)) => path,
            _ => panic!("expected a workspace path target"),
        };
        assert_eq!(
            path,
            trees::domain::CanonicalPath::resolve(".").expect("current directory should resolve")
        );

        let cwd_arguments = trees::cli::ReleaseArgs {
            workspace_dir: None,
            claim_id: None,
        };
        assert!(matches!(
            release_target(&cwd_arguments),
            Ok(trees::workspace::ReleaseTarget::CurrentDirectory(_))
        ));

        let claim_id = trees::domain::ClaimId::new();
        let claim_arguments = trees::cli::ReleaseArgs {
            workspace_dir: None,
            claim_id: Some(claim_id.to_string()),
        };
        assert!(matches!(
            release_target(&claim_arguments),
            Ok(trees::workspace::ReleaseTarget::ClaimId(value)) if value == claim_id
        ));
    }

    #[test]
    fn rejects_an_invalid_claim_release_target() {
        let invalid_claim = trees::cli::ReleaseArgs {
            workspace_dir: None,
            claim_id: Some("invalid".to_owned()),
        };
        assert!(claim_release_target(
            invalid_claim
                .claim_id
                .as_deref()
                .expect("claim ID should exist")
        )
        .is_err());
    }

    #[test]
    fn formats_release_output_with_workspace_id() {
        let workspace_id = trees::domain::WorkspaceId::new();
        let claim_id = trees::domain::ClaimId::new();
        let result = trees::workspace::ReleaseResult {
            workspace_id,
            workspace_path: trees::domain::CanonicalPath::from_absolute("/tmp/workspace")
                .expect("workspace path should be absolute"),
            claim_id,
            released_at: trees::domain::Timestamp::parse("2026-09-09T00:00:00Z")
                .expect("release timestamp should parse"),
        };

        assert_eq!(
            release_output(&result),
            format!(
                "workspace_id={workspace_id}\nworkspace_path=/tmp/workspace\nclaim_id={claim_id}\nreleased_at=2026-09-09T00:00:00Z"
            )
        );
    }
}
