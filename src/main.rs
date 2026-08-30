use std::process::{ExitCode, ExitStatus};

use clap::Parser;

fn main() -> ExitCode {
    run(trees::cli::Cli::parse())
}

fn run(cli: trees::cli::Cli) -> ExitCode {
    match cli.command {
        trees::cli::Command::Create(arguments) => run_create(arguments),
        trees::cli::Command::Checkin(arguments) => run_checkin(arguments),
        trees::cli::Command::Codex(arguments) => run_codex(arguments),
    }
}

fn run_create(arguments: trees::cli::CreateArgs) -> ExitCode {
    let trees::cli::CreateArgs {
        workspace_path,
        repositories,
        checkout_id,
    } = arguments;
    match workspace_path {
        Some(workspace_path) => {
            if checkout_id.is_some() {
                eprintln!("Error: checkout ID is only valid for automatic create");
                return ExitCode::FAILURE;
            }
            match trees::workspace::create(trees::workspace::CreateRequest {
                workspace_path,
                repositories,
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
        None => run_automatic_create(repositories, checkout_id),
    }
}

fn run_automatic_create(
    repositories: Vec<std::path::PathBuf>,
    checkout_id: Option<String>,
) -> ExitCode {
    let checkout_id = match checkout_id {
        Some(value) => match value.parse::<trees::domain::CheckoutId>() {
            Ok(checkout_id) => Some(checkout_id),
            Err(error) => {
                eprintln!("Error: invalid checkout ID: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let plan =
        match trees::workspace::prepare_automatic(&trees::workspace::AutomaticCreateRequest {
            repositories,
            checkout_id,
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
            print_automatic_checkout_result(&result);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_checkin(arguments: trees::cli::CheckinArgs) -> ExitCode {
    let checkout_id = match arguments.checkout_id.parse::<trees::domain::CheckoutId>() {
        Ok(checkout_id) => checkout_id,
        Err(error) => {
            eprintln!("Error: invalid checkout ID: {error}");
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
    match trees::workspace::checkin_automatic(&mut connection, &workspace_path, checkout_id) {
        Ok(result) => {
            println!("workspace_path={}", result.workspace_path);
            println!("checkout_id={}", result.checkout_id);
            println!("checked_in_at={}", result.checked_in_at);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_automatic_checkout_result(result: &trees::workspace::AutomaticCheckoutResult) {
    println!("workspace_path={}", result.workspace_path);
    println!("pool_key={}", result.pool_key);
    println!("checkout_id={}", result.checkout_id);
    println!("lease_expires_at={}", result.lease_expires_at);
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
}
