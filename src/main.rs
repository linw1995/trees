use std::io::{self, IsTerminal, Write};
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
        trees::cli::Command::Codex(arguments) => run_codex(arguments),
    }
}

fn run_create(arguments: trees::cli::CreateArgs) -> ExitCode {
    let trees::cli::CreateArgs {
        workspace_path,
        repositories,
        json,
    } = arguments;
    match workspace_path {
        Some(workspace_path) => {
            match trees::workspace::create(trees::workspace::CreateRequest {
                workspace_path,
                repositories,
            }) {
                Ok(result) => {
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
        None => run_automatic_create(repositories, json),
    }
}

fn run_automatic_create(repositories: Vec<std::path::PathBuf>, json: bool) -> ExitCode {
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
    match trees::workspace::allocate_automatic_workspace(&mut connection, &plan) {
        Ok(result) => {
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

fn run_release(arguments: trees::cli::ReleaseArgs) -> ExitCode {
    let claim_id = match arguments.claim_id.parse::<trees::domain::ClaimId>() {
        Ok(claim_id) => claim_id,
        Err(error) => {
            eprintln!("Error: invalid claim ID: {error}");
            return ExitCode::FAILURE;
        }
    };
    let workspace_path = match trees::validation::resolve_workspace_path(&arguments.workspace_path)
    {
        Ok(path) => path,
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
    match trees::workspace::release_automatic_workspace(&mut connection, &workspace_path, claim_id)
    {
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
    let workspace_root = match trees::paths::managed_workspace_directory() {
        Ok(path) => match trees::domain::CanonicalPath::from_absolute(path) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
        },
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut connection = match trees::database::open_read_only() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let scan = match trees::gc::scan(&mut connection, &workspace_root, arguments.older_than) {
        Ok(scan) => scan,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("cutoff={}", scan.cutoff);
    println!("automatic={}", scan.counts.automatic);
    println!("unclaimed={}", scan.counts.unclaimed);
    println!("claimed={}", scan.counts.claimed);
    println!("age_eligible={}", scan.counts.age_eligible);
    println!("safe_to_reclaim={}", scan.counts.safe_to_reclaim);
    println!(
        "candidates={}",
        scan.execution_candidate_count(arguments.force)
    );
    for candidate in &scan.candidates {
        println!(
            "candidate={} reason={}",
            candidate.workspace.canonical_path,
            candidate.reason()
        );
    }
    if arguments.dry_run {
        return ExitCode::SUCCESS;
    }
    let candidate_count = scan.execution_candidate_count(arguments.force);
    if arguments.force {
        eprintln!("Warning: --force may remove dirty worktrees and unexpected workspace content.");
    } else if !arguments.yes && candidate_count > 0 {
        if !io::stdin().is_terminal() {
            eprintln!(
                "Error: interactive confirmation is unavailable; use --dry-run, --yes, or --force"
            );
            return ExitCode::FAILURE;
        }
        print!("Reclaim {candidate_count} workspaces? [y/N] ");
        if let Err(error) = io::stdout().flush() {
            eprintln!("Error: failed to flush confirmation prompt: {error}");
            return ExitCode::FAILURE;
        }
        let mut answer = String::new();
        if let Err(error) = io::stdin().read_line(&mut answer) {
            eprintln!("Error: failed to read confirmation: {error}");
            return ExitCode::FAILURE;
        }
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("cancelled=true");
            return ExitCode::SUCCESS;
        }
    }

    drop(connection);
    let mut connection = match trees::database::open_default() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let report = match trees::gc::execute(
        &mut connection,
        &workspace_root,
        arguments.older_than,
        arguments.force,
    ) {
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

fn print_automatic_claim_result(result: &trees::workspace::AutomaticClaimResult) {
    println!("{}", automatic_claim_shell_output(result));
}

fn automatic_claim_shell_output(result: &trees::workspace::AutomaticClaimResult) -> String {
    format!(
        "WORKSPACE_PATH={}\nPOOL_KEY={}\nCLAIM_ID={}",
        bash_quote(&result.workspace_path.to_string()),
        bash_quote(&result.pool_key.to_string()),
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
            pool_key: trees::domain::PoolId::new(),
            claim_id: trees::domain::ClaimId::new(),
        };

        let output = automatic_claim_shell_output(&result);

        assert!(output.starts_with("WORKSPACE_PATH='/tmp/workspace'\nPOOL_KEY='"));
        assert!(output.contains("\nCLAIM_ID='"));
    }

    #[test]
    fn serializes_automatic_claim_as_json() {
        let result = trees::workspace::AutomaticClaimResult {
            workspace_path: trees::domain::CanonicalPath::from_absolute("/tmp/workspace")
                .expect("workspace path should be absolute"),
            pool_key: trees::domain::PoolId::new(),
            claim_id: trees::domain::ClaimId::new(),
        };
        let output = serde_json::to_string(&result).expect("claim should serialize as JSON");
        let value: serde_json::Value =
            serde_json::from_str(&output).expect("claim JSON should be valid");

        assert_eq!(value["workspace_path"], "/tmp/workspace");
        let pool_key = value["pool_key"]
            .as_str()
            .expect("pool key should be a string");
        assert!(pool_key.parse::<trees::domain::PoolId>().is_ok());
        assert!(value["claim_id"].is_string());
    }
}
