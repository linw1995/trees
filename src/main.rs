use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{ExitCode, ExitStatus};

use clap::Parser;

fn main() -> ExitCode {
    run(trees::cli::Cli::parse())
}

fn run(cli: trees::cli::Cli) -> ExitCode {
    match cli.command {
        trees::cli::Command::Create(arguments) => run_create(arguments),
        trees::cli::Command::Release(arguments) => run_release(arguments),
        trees::cli::Command::Config(arguments) => run_config(arguments),
        trees::cli::Command::Gc(arguments) => run_gc(arguments),
        trees::cli::Command::Status(arguments) => run_status(arguments),
        trees::cli::Command::Codex(arguments) => run_codex(arguments),
    }
}

fn run_create(arguments: trees::cli::CreateArgs) -> ExitCode {
    let trees::cli::CreateArgs {
        workspace_path,
        repositories,
        json,
        open,
    } = arguments;
    let open = match resolve_open_program(open) {
        Ok(open) => open,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    match workspace_path {
        Some(workspace_path) => {
            match trees::workspace::create(trees::workspace::CreateRequest {
                workspace_path,
                repositories,
            }) {
                Ok(result) => {
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
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        None => run_automatic_create(repositories, json, open.as_deref()),
    }
}

fn resolve_open_program(open: Option<Option<OsString>>) -> Result<Option<OsString>, String> {
    match open {
        None => Ok(None),
        Some(Some(program)) if !program.is_empty() => Ok(Some(program)),
        Some(Some(_)) => Err("--open program must not be empty".to_owned()),
        Some(None) => std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .map(Some)
            .ok_or_else(|| "$SHELL is unset or empty; use --open=<PROGRAM>".to_owned()),
    }
}

fn run_automatic_create(
    repositories: Vec<std::path::PathBuf>,
    json: bool,
    open: Option<&OsStr>,
) -> ExitCode {
    let plan =
        match trees::workspace::prepare_automatic(&trees::workspace::AutomaticCreateRequest {
            repositories,
        }) {
            Ok(plan) => plan,
            Err(error) => {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
        };
    let mut connection = match trees::database::open_default() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    run_automatic_allocation(&mut connection, &plan, json, open)
}

fn run_automatic_allocation(
    connection: &mut diesel::sqlite::SqliteConnection,
    plan: &trees::workspace::AutomaticAllocationPlan,
    json: bool,
    open: Option<&OsStr>,
) -> ExitCode {
    match trees::workspace::allocate_automatic_workspace(connection, plan) {
        Ok(result) => {
            if let Some(program) = open {
                return open_workspace(program, result.workspace_path.as_path());
            }
            if json {
                print_json(&result)
            } else {
                print_automatic_claim_result(&result);
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(unix)]
fn open_workspace(program: &OsStr, workspace_path: &Path) -> ExitCode {
    use std::os::unix::process::CommandExt;

    let error = std::process::Command::new(program)
        .current_dir(workspace_path)
        .exec();
    eprintln!(
        "Error: failed to open {} in workspace {}: {}",
        Path::new(program).display(),
        workspace_path.display(),
        error
    );
    ExitCode::FAILURE
}

#[cfg(not(unix))]
fn open_workspace(program: &OsStr, workspace_path: &Path) -> ExitCode {
    match std::process::Command::new(program)
        .current_dir(workspace_path)
        .status()
    {
        Ok(status) => exit_code(status),
        Err(error) => {
            eprintln!(
                "Error: failed to open {} in workspace {}: {}",
                Path::new(program).display(),
                workspace_path.display(),
                error
            );
            ExitCode::FAILURE
        }
    }
}

fn run_release(arguments: trees::cli::ReleaseArgs) -> ExitCode {
    match release_automatic(&arguments) {
        Ok(result) => {
            println!("workspace_path={}", result.workspace_path);
            println!("claim_id={}", result.claim_id);
            println!("released_at={}", result.released_at);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn release_automatic(
    arguments: &trees::cli::ReleaseArgs,
) -> Result<trees::workspace::ReleaseResult, String> {
    let target = release_target(arguments)?;
    let mut connection = trees::database::open_default().map_err(|error| error.to_string())?;
    trees::workspace::release_automatic_workspace_by_target(&mut connection, target)
        .map_err(|error| error.to_string())
}

fn release_target(
    arguments: &trees::cli::ReleaseArgs,
) -> Result<trees::workspace::ReleaseTarget, String> {
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
) -> Result<trees::workspace::ReleaseTarget, String> {
    trees::validation::resolve_workspace_path(path)
        .map(trees::workspace::ReleaseTarget::WorkspacePath)
        .map_err(|error| error.to_string())
}

fn current_directory_release_target() -> Result<trees::workspace::ReleaseTarget, String> {
    trees::domain::CanonicalPath::resolve(".")
        .map(trees::workspace::ReleaseTarget::CurrentDirectory)
        .map_err(|error| error.to_string())
}

fn claim_release_target(claim_id: &str) -> Result<trees::workspace::ReleaseTarget, String> {
    claim_id
        .parse::<trees::domain::ClaimId>()
        .map(trees::workspace::ReleaseTarget::ClaimId)
        .map_err(|error| format!("invalid claim ID: {error}"))
}

fn run_config(arguments: trees::cli::ConfigArgs) -> ExitCode {
    match arguments.command {
        trees::cli::ConfigCommand::Set(arguments) => match arguments.setting {
            trees::cli::ConfigSetting::WorkspacesDir => {
                match trees::config::set_workspaces_directory(&arguments.value) {
                    Ok(path) => {
                        println!("workspaces_dir={}", path.display());
                        ExitCode::SUCCESS
                    }
                    Err(error) => {
                        eprintln!("Error: {error}");
                        ExitCode::FAILURE
                    }
                }
            }
        },
    }
}

fn run_gc(arguments: trees::cli::GcArgs) -> ExitCode {
    match run_gc_command(&arguments) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_gc_command(arguments: &trees::cli::GcArgs) -> Result<ExitCode, String> {
    let scan = load_gc_scan(arguments)?;
    print_gc_scan(&scan, arguments.force);
    run_gc_after_scan(arguments, &scan)
}

fn run_gc_after_scan(
    arguments: &trees::cli::GcArgs,
    scan: &trees::gc::GcScan,
) -> Result<ExitCode, String> {
    if arguments.dry_run {
        return Ok(ExitCode::SUCCESS);
    }
    match confirm_gc(
        arguments.force,
        arguments.yes,
        scan.execution_candidate_count(arguments.force),
    ) {
        Ok(GcConfirmation::Proceed) => Ok(execute_gc(arguments)),
        Ok(GcConfirmation::Cancelled) => Ok(ExitCode::SUCCESS),
        Err(exit_code) => Ok(exit_code),
    }
}

fn load_gc_scan(arguments: &trees::cli::GcArgs) -> Result<trees::gc::GcScan, String> {
    let mut connection = trees::database::open_read_only().map_err(|error| error.to_string())?;
    trees::gc::scan_with_force(&mut connection, arguments.older_than, arguments.force)
        .map_err(|error| error.to_string())
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

fn confirm_gc(force: bool, yes: bool, candidate_count: usize) -> Result<GcConfirmation, ExitCode> {
    if force {
        eprintln!("Warning: --force may remove dirty worktrees and unexpected workspace content.");
    } else if yes || candidate_count == 0 {
        return Ok(GcConfirmation::Proceed);
    } else {
        return confirm_gc_interactively(candidate_count);
    }
    Ok(GcConfirmation::Proceed)
}

fn confirm_gc_interactively(candidate_count: usize) -> Result<GcConfirmation, ExitCode> {
    if !io::stdin().is_terminal() {
        eprintln!(
            "Error: interactive confirmation is unavailable; use --dry-run, --yes, or --force"
        );
        return Err(ExitCode::FAILURE);
    }
    print!("Reclaim {candidate_count} workspaces? [y/N] ");
    if let Err(error) = io::stdout().flush() {
        eprintln!("Error: failed to flush confirmation prompt: {error}");
        return Err(ExitCode::FAILURE);
    }
    let mut answer = String::new();
    if let Err(error) = io::stdin().read_line(&mut answer) {
        eprintln!("Error: failed to read confirmation: {error}");
        return Err(ExitCode::FAILURE);
    }
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        println!("cancelled=true");
        return Ok(GcConfirmation::Cancelled);
    }
    Ok(GcConfirmation::Proceed)
}

fn execute_gc(arguments: &trees::cli::GcArgs) -> ExitCode {
    let report = match execute_gc_report(arguments) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("unclaimed={}", report.scan.counts.unclaimed);
    println!("claimed={}", report.scan.counts.claimed);
    println!("reclaimed={}", report.reclaimed.len());
    println!("skipped={}", report.skipped.len());
    println!("failed={}", report.failed.len());
    if !report.failed.is_empty() {
        for failure in report.failed {
            eprintln!("GC failed: {}: {}", failure.workspace_path, failure.error);
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn execute_gc_report(
    arguments: &trees::cli::GcArgs,
) -> Result<trees::gc::GcExecutionReport, String> {
    let mut connection = trees::database::open_default().map_err(|error| error.to_string())?;
    trees::gc::execute(&mut connection, arguments.older_than, arguments.force)
        .map_err(|error| error.to_string())
}

fn run_status(arguments: trees::cli::StatusArgs) -> ExitCode {
    if arguments.all && arguments.view != trees::cli::StatusView::Workspaces {
        eprintln!("Error: --all requires --view workspaces");
        return ExitCode::FAILURE;
    }
    match arguments.view {
        trees::cli::StatusView::Pools => run_pool_status(arguments.json),
        trees::cli::StatusView::Workspaces => run_workspace_status(arguments.all, arguments.json),
    }
}

fn run_pool_status(json: bool) -> ExitCode {
    let snapshot = match load_pool_status_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    if json {
        print_json(&snapshot)
    } else {
        println!("{}", trees::status::render_pools_human(&snapshot));
        ExitCode::SUCCESS
    }
}

fn load_pool_status_snapshot() -> Result<trees::status::PoolStatusSnapshot, String> {
    let mut connection = match trees::database::open_read_only() {
        Ok(connection) => connection,
        Err(trees::database::DatabaseError::ReadOnlyDatabaseMissing(_)) => {
            return Ok(trees::status::PoolStatusSnapshot::empty());
        }
        Err(error) => return Err(error.to_string()),
    };
    trees::status::load_pool_snapshot(&mut connection)
        .map_err(|error| format!("failed to load workspace pool status: {error}"))
}

fn run_workspace_status(include_reclaimed: bool, json: bool) -> ExitCode {
    let snapshot = match load_workspace_status_snapshot(include_reclaimed) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    if json {
        print_json(&snapshot)
    } else {
        println!("{}", trees::status::render_workspaces_human(&snapshot));
        ExitCode::SUCCESS
    }
}

fn load_workspace_status_snapshot(
    include_reclaimed: bool,
) -> Result<trees::status::StatusSnapshot, String> {
    let mut connection = match trees::database::open_read_only() {
        Ok(connection) => connection,
        Err(trees::database::DatabaseError::ReadOnlyDatabaseMissing(_)) => {
            return Ok(trees::status::StatusSnapshot::empty());
        }
        Err(error) => return Err(error.to_string()),
    };
    trees::status::load_snapshot(&mut connection, include_reclaimed)
        .map_err(|error| format!("failed to load workspace status: {error}"))
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

fn print_json<T: serde::Serialize>(value: &T) -> ExitCode {
    match serde_json::to_string(value) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Error: failed to serialize JSON output: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_codex(arguments: trees::cli::CodexArgs) -> ExitCode {
    let trees::cli::CodexArgs {
        codex_bin,
        subcommand,
        codex_args,
    } = arguments;

    let native_args = match &subcommand {
        None => &codex_args,
        Some(trees::cli::CodexSubcommand::Resume(resume)) => &resume.codex_args,
    };
    let workspace_path = match trees::codex::args::workspace_path_from_codex_args(native_args) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };

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

    match result {
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

        assert_eq!(run(cli), ExitCode::FAILURE);
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
        assert_eq!(resolved, Ok(Some(OsString::from("/bin/test-shell"))));
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
            resolved,
            Err("$SHELL is unset or empty; use --open=<PROGRAM>".to_owned())
        );
    }

    #[test]
    fn rejects_an_empty_explicit_open_program() {
        assert_eq!(
            resolve_open_program(Some(Some(OsString::new()))),
            Err("--open program must not be empty".to_owned())
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
}
