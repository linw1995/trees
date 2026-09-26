use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
#[cfg(not(unix))]
use std::process::ExitStatus;

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
        trees::cli::Command::Add(arguments) => run_add(arguments),
        trees::cli::Command::Claim(arguments) => run_claim(arguments),
        trees::cli::Command::Release(arguments) => run_release(arguments),
        trees::cli::Command::Config(arguments) => run_config(arguments),
        trees::cli::Command::Gc(arguments) => run_gc(arguments),
        trees::cli::Command::Remove(arguments) => run_remove(arguments),
        trees::cli::Command::Status(arguments) => run_status(arguments),
        trees::cli::Command::Open(arguments) => run_open(arguments),
    }
}

fn run_add(arguments: trees::cli::AddArgs) -> Result<ExitCode, CliError> {
    let selector = arguments.selector()?;
    let mut connection = trees::database::open_default()?;
    let result = trees::add::execute(
        &mut connection,
        trees::add::AddRequest {
            selector,
            repositories: arguments.repositories,
            offline: arguments.offline,
        },
    )?;
    if arguments.json {
        return print_json(&result);
    }
    println!("operation_id={}", result.operation_id);
    println!("workspace_id={}", result.workspace_id);
    println!("workspace_path={}", result.workspace_path);
    if let Some(claim) = result.claim_id {
        println!("claim_id={claim}");
    }
    if let Some(pool) = result.previous_pool_id {
        println!("previous_pool_id={pool}");
    }
    if let Some(pool) = result.pool_id {
        println!("pool_id={pool}");
    }
    for repo in result.repositories {
        let outcome = match repo.result {
            trees::add::RepositoryOutcome::Added => "added",
            trees::add::RepositoryOutcome::AlreadyPresent => "already_present",
        };
        println!(
            "repo_result={outcome} origin_repository_id={} worktree_id={} worktree_path={}",
            repo.origin_repository_id, repo.worktree_id, repo.worktree_path
        );
    }
    for moved in result.relocated {
        println!(
            "relocated_worktree_id={} previous_path={} worktree_path={}",
            moved.worktree_id, moved.previous_path, moved.worktree_path
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn run_create(arguments: trees::cli::CreateArgs) -> Result<ExitCode, CliError> {
    let trees::cli::CreateArgs {
        workspace_path,
        repositories,
        json,
        offline,
        open,
        open_args,
        release_on_exit,
    } = arguments;
    let open = resolve_open_program(open)?;
    if let Some(path) = &workspace_path {
        trees::validation::validate_new_workspace_target(path)?;
    }
    let expected =
        trees::origin::resolve::resolve(&repositories, offline, workspace_path.as_deref())?;
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
                return open_workspace(program, &open_args, result.workspace_path.as_path());
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
        None => run_automatic_create(
            repositories,
            json,
            offline,
            open.as_deref(),
            &open_args,
            release_on_exit,
            &expected,
        ),
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
    open_args: &[OsString],
    release_on_exit: bool,
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
    let result = trees::workspace::allocate_automatic_workspace(&mut connection, &plan)?;
    drop(connection);
    if let Some(program) = open {
        if release_on_exit {
            let identity = trees::workspace_session::SessionIdentity::from(result);
            print_open_notice(program, identity.workspace_path.as_path());
            let report = trees::workspace_session::run(&identity, program, open_args, |event| {
                use trees::workspace_session::SessionEvent;
                match event {
                    SessionEvent::InitialFailed(error) => eprintln!("[trees] Error: {error}"),
                    SessionEvent::ReleaseFailed(error) => {
                        eprintln!("[trees] Release failed: {error}")
                    }
                    SessionEvent::Recovering => eprintln!(
                        "[trees] Opening $SHELL in {}. Exiting the shell will retry release.",
                        identity.workspace_path
                    ),
                }
            });
            if let Err(error) = &report.cleanup {
                eprintln!("[trees] Error: {error}");
                eprintln!(
                    "[trees] Workspace: {}\n[trees] Claim: {}\n[trees] Manual recovery: trees release --claim-id {}",
                    identity.workspace_path, identity.claim_id, identity.claim_id
                );
            } else {
                eprintln!("[trees] Claim released: {}", identity.claim_id);
            }
            return Ok(ExitCode::from(report.exit_code()));
        }
        return open_workspace(program, open_args, result.workspace_path.as_path());
    }
    if json {
        print_json(&result)
    } else {
        print_automatic_claim_result(&result);
        Ok(ExitCode::SUCCESS)
    }
}

fn print_open_notice(program: &OsStr, workspace_path: &Path) {
    eprintln!(
        "[trees] Opening {} in {}",
        program.to_string_lossy(),
        workspace_path.display()
    );
}

#[cfg(unix)]
fn open_workspace(
    program: &OsStr,
    args: &[OsString],
    workspace_path: &Path,
) -> Result<ExitCode, CliError> {
    use std::os::unix::process::CommandExt;

    print_open_notice(program, workspace_path);
    let error = std::process::Command::new(program)
        .args(args)
        .current_dir(workspace_path)
        .exec();
    Err(CliError::OpenWorkspace {
        program: PathBuf::from(program),
        workspace_path: workspace_path.to_owned(),
        source: error,
    })
}

fn run_open(arguments: trees::cli::OpenArgs) -> Result<ExitCode, CliError> {
    let selector = arguments.selector()?;
    let program = resolve_required_program(arguments.program, "--program")?;
    let mut connection = trees::database::open_read_only()?;
    let workspace_path = match arguments.workspace_id {
        Some(id) => trees::workspace_open::resolve_id(&mut connection, id)?,
        None => trees::workspace_open::resolve_selector(&mut connection, &selector)?,
    };
    drop(connection);
    open_workspace(&program, &[], workspace_path.as_path())
}

#[cfg(not(unix))]
fn open_workspace(
    program: &OsStr,
    args: &[OsString],
    workspace_path: &Path,
) -> Result<ExitCode, CliError> {
    print_open_notice(program, workspace_path);
    match std::process::Command::new(program)
        .args(args)
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

fn run_claim(arguments: trees::cli::ClaimArgs) -> Result<ExitCode, CliError> {
    let target = arguments.selector()?;
    let result = {
        let mut connection = trees::database::open_default()?;
        trees::workspace::claim_automatic_workspace(&mut connection, target)?
    };
    let output = if arguments.json {
        serde_json::to_string(&result).context(SerializeJsonSnafu)?
    } else {
        format!(
            "WORKSPACE_ID={}\nWORKSPACE_PATH={}\nPOOL_ID={}\nCLAIM_ID={}",
            bash_quote(&result.workspace_id.to_string()),
            bash_quote(&result.workspace_path.to_string()),
            bash_quote(&result.pool_id.to_string()),
            bash_quote(&result.claim_id.to_string()),
        )
    };
    writeln!(io::stdout().lock(), "{output}").context(ClaimOutputSnafu)?;
    Ok(ExitCode::SUCCESS)
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
    let target = arguments.selector()?;
    let mut connection = trees::database::open_default()?;
    Ok(trees::workspace::release_automatic_workspace_by_target(
        &mut connection,
        target,
    )?)
}

fn run_config(arguments: trees::cli::ConfigArgs) -> Result<ExitCode, CliError> {
    match arguments.command {
        trees::cli::ConfigCommand::Path => {
            let path = trees::paths::configuration_path()?;
            println!("{}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        trees::cli::ConfigCommand::Show(arguments) => {
            let config = trees::config::effective_configuration()?;
            if arguments.json {
                return print_json(&serde_json::json!({
                    "workspaces_dir": config.workspaces_dir.to_string_lossy(),
                    "origins_dir": config.origins_dir.to_string_lossy(),
                    "latest_session_hook": config.latest_session_hook.as_ref().map(|hook| {
                        serde_json::json!({
                            "program": hook.program.to_string_lossy(),
                            "timeout_ms": hook.timeout.as_millis(),
                        })
                    }),
                }));
            }
            println!(
                "workspaces_dir={}",
                bash_quote(&config.workspaces_dir.to_string_lossy())
            );
            println!(
                "origins_dir={}",
                bash_quote(&config.origins_dir.to_string_lossy())
            );
            let (program, timeout_ms) = match &config.latest_session_hook {
                Some(hook) => (
                    hook.program.to_string_lossy(),
                    hook.timeout.as_millis().to_string(),
                ),
                None => (Default::default(), String::new()),
            };
            println!("latest_session_hook_program={}", bash_quote(&program));
            println!("latest_session_hook_timeout_ms={}", bash_quote(&timeout_ms));
            Ok(ExitCode::SUCCESS)
        }
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
    println!("safe_to_remove={}", scan.counts.safe_to_remove);
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
    print!("Remove {candidate_count} workspaces? [y/N] ");
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
    println!("removed={}", report.removed.len());
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
    let target = {
        let mut connection = trees::database::open_read_only()?;
        trees::storage::removal::resolve(&mut connection, arguments.workspace_id)?
    };
    match target {
        trees::storage::removal::RemovalTarget::Workspace(_) => run_remove_command(&arguments),
        trees::storage::removal::RemovalTarget::Origin(row) => run_origin_removal(&arguments, &row),
    }
}

fn run_origin_removal(
    arguments: &trees::cli::RemoveArgs,
    row: &trees::storage::OriginRepositoryRow,
) -> Result<ExitCode, CliError> {
    println!("repo_id={}", row.id);
    println!("repo_path={}", row.source_path.to_string().escape_debug());
    println!("action=remove_repository_record");
    let references = {
        let mut connection = trees::database::open_read_only()?;
        trees::storage::removal::references(&mut connection, row.id).context(RepoStatusSnafu)?
    };
    println!("worktree_references={}", references.worktrees);
    println!("pool_references={}", references.pools);
    if arguments.dry_run {
        return Ok(ExitCode::SUCCESS);
    }
    if references.any() {
        eprintln!(
            "Repository is still referenced; retained history and pool membership also count."
        );
        return Ok(ExitCode::FAILURE);
    }
    if matches!(
        confirm_entity_removal(arguments.force, arguments.yes, true)?,
        GcConfirmation::Cancelled
    ) {
        return Ok(ExitCode::SUCCESS);
    }
    let mut connection = trees::database::open_default()?;
    trees::storage::removal::remove_origin(&mut connection, row)?;
    println!("removed=true");
    Ok(ExitCode::SUCCESS)
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
    confirm_entity_removal(force, yes, false)
}

fn confirm_entity_removal(
    force: bool,
    yes: bool,
    repository: bool,
) -> Result<GcConfirmation, CliError> {
    if force && repository {
        return Ok(GcConfirmation::Proceed);
    }
    if force {
        eprintln!("Warning: --force may remove claimed workspaces, dirty worktrees, and unexpected workspace content.");
        return Ok(GcConfirmation::Proceed);
    }
    if yes {
        return Ok(GcConfirmation::Proceed);
    }
    if !io::stdin().is_terminal() {
        return InteractiveConfirmationUnavailableSnafu.fail();
    }
    print!(
        "{} [y/N] ",
        if repository {
            "Remove this repository record (keep source files)?"
        } else {
            "Remove this workspace?"
        }
    );
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
    if arguments.all && arguments.view != trees::cli::StatusView::Workspaces {
        return InvalidStatusArgumentsSnafu.fail();
    }
    let selector = arguments.selector().map_err(|error| match error {
        trees::cli::workspace_locator::InputError::Directory { source } => {
            CliError::from(trees::status::target::TargetError::Directory { source })
        }
        error => CliError::from(error),
    })?;
    let view = match arguments.view {
        trees::cli::StatusView::Pools => trees::status::StatusView::Pools,
        trees::cli::StatusView::Workspaces => trees::status::StatusView::Workspaces,
        trees::cli::StatusView::Repos => trees::status::StatusView::Repos,
    };
    let connection = match trees::database::open_read_only() {
        Ok(connection) => Some(connection),
        Err(trees::database::DatabaseError::ReadOnlyDatabaseMissing { .. }) => None,
        Err(source) => return Err(source.into()),
    };
    let report = trees::status::report::load(
        connection,
        &selector,
        view,
        arguments.all,
        arguments.no_hooks,
    )?;
    if arguments.json {
        print_json(&report)
    } else {
        println!(
            "{}",
            trees::status::summary::render_with_sessions(
                &report.snapshot,
                &selector,
                status_color_enabled(),
                report.target_processes.as_ref(),
                report.workspace_sessions.as_ref()
            )
        );
        Ok(ExitCode::SUCCESS)
    }
}

fn status_color_enabled() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
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

#[cfg(not(unix))]
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
    Addition { source: trees::add::AddError },
    #[snafu(transparent)]
    RemovalTarget {
        source: trees::storage::removal::TargetError,
    },
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
    #[snafu(transparent)]
    WorkspaceInput {
        source: trees::cli::workspace_locator::InputError,
    },
    #[snafu(transparent)]
    Config { source: trees::config::ConfigError },
    #[snafu(transparent)]
    Path { source: trees::paths::PathError },
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
    #[snafu(display("--all requires --view workspaces"))]
    InvalidStatusArguments,
    #[snafu(display("failed to load repository status: {source}"))]
    RepoStatus { source: diesel::result::Error },
    #[snafu(transparent)]
    StatusTarget {
        source: trees::status::target::TargetError,
    },
    #[snafu(transparent)]
    StatusSnapshot {
        source: trees::status::combined::SnapshotError,
    },
    #[snafu(display(
        "claim committed but output failed; inspect trees status for its identity: {source}"
    ))]
    ClaimOutput { source: io::Error },
    #[snafu(display("failed to serialize JSON output: {source}"))]
    SerializeJson { source: serde_json::Error },
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
