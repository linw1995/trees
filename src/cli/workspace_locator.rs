use std::path::PathBuf;

use clap::{ArgGroup, Args};
use snafu::{ResultExt, Snafu};

use crate::domain::{CanonicalPath, CanonicalPathError, ClaimId, IdentifierError, WorkspaceId};
use crate::validation::{self, ValidationError};
use crate::workspace_locator::WorkspaceSelector;

pub const GROUP: &str = "workspace_locator";

#[derive(Debug, Clone, Copy)]
pub enum SelectionDefault {
    RequireExplicit,
    CurrentDirectory,
}

pub fn group(default: SelectionDefault) -> ArgGroup {
    ArgGroup::new(GROUP)
        .multiple(false)
        .required(matches!(default, SelectionDefault::RequireExplicit))
}

#[derive(Debug, Default, Args)]
#[group(skip)]
pub struct WorkspaceLocatorArgs {
    #[arg(long, group = GROUP, value_name = "WORKSPACE_ID", help = "Select a workspace by ID")]
    pub workspace_id: Option<WorkspaceId>,

    #[arg(long, group = GROUP, value_name = "WORKSPACE_DIR", help = "Select an exact workspace root")]
    pub workspace_dir: Option<PathBuf>,

    #[arg(long, group = GROUP, value_name = "CLAIM_ID", help = "Select a workspace by its active claim")]
    pub claim_id: Option<String>,
}

#[derive(Debug)]
pub enum WorkspaceLocatorInput {
    Id(WorkspaceId),
    ExactPath(PathBuf),
    ClaimId(ClaimId),
}

#[derive(Debug, Snafu)]
pub enum InputError {
    #[snafu(display("workspace selectors are mutually exclusive"))]
    Conflict,
    #[snafu(display("an explicit workspace selector is required"))]
    Required,
    #[snafu(display("invalid claim ID: {source}"))]
    ClaimId { source: IdentifierError },
    #[snafu(transparent)]
    Validation { source: ValidationError },
    #[snafu(context(false), display("{source}"))]
    Directory { source: CanonicalPathError },
}

impl WorkspaceLocatorArgs {
    pub fn resolve(
        &self,
        legacy: Option<WorkspaceLocatorInput>,
        default: SelectionDefault,
    ) -> Result<WorkspaceSelector, InputError> {
        self.resolve_with_directory(legacy, default, || CanonicalPath::resolve("."))
    }

    fn resolve_with_directory(
        &self,
        legacy: Option<WorkspaceLocatorInput>,
        default: SelectionDefault,
        directory: impl FnOnce() -> Result<CanonicalPath, CanonicalPathError>,
    ) -> Result<WorkspaceSelector, InputError> {
        let count = usize::from(legacy.is_some())
            + usize::from(self.workspace_id.is_some())
            + usize::from(self.workspace_dir.is_some())
            + usize::from(self.claim_id.is_some());
        snafu::ensure!(count <= 1, ConflictSnafu);
        let input = match legacy {
            Some(input) => Some(input),
            None => match (&self.workspace_id, &self.workspace_dir, &self.claim_id) {
                (Some(id), _, _) => Some(WorkspaceLocatorInput::Id(*id)),
                (_, Some(path), _) => Some(WorkspaceLocatorInput::ExactPath(path.clone())),
                (_, _, Some(id)) => Some(WorkspaceLocatorInput::ClaimId(
                    id.parse().context(ClaimIdSnafu)?,
                )),
                _ => None,
            },
        };
        match input {
            Some(WorkspaceLocatorInput::Id(id)) => Ok(WorkspaceSelector::Id(id)),
            Some(WorkspaceLocatorInput::ClaimId(id)) => Ok(WorkspaceSelector::ClaimId(id)),
            Some(WorkspaceLocatorInput::ExactPath(path)) => Ok(WorkspaceSelector::ExactPath(
                validation::resolve_workspace_path(&path)?,
            )),
            None => match default {
                SelectionDefault::RequireExplicit => RequiredSnafu.fail(),
                SelectionDefault::CurrentDirectory => {
                    Ok(WorkspaceSelector::ContainingDirectory(directory()?))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_enforces_the_shared_contract_for_every_command() {
        use crate::cli::{Cli, Command};
        use clap::{CommandFactory, Parser};

        Cli::command().debug_assert();
        let id = WorkspaceId::new().to_string();
        let claim = ClaimId::new().to_string();
        for command in ["status", "open", "release"] {
            let positional = if command == "release" { "." } else { &id };
            let inputs = [
                vec![positional],
                vec!["--workspace-id", &id],
                vec!["--workspace-dir", "."],
                vec!["--claim-id", &claim],
            ];
            for (index, input) in inputs.iter().enumerate() {
                let mut args = vec!["trees", command];
                args.extend(input);
                let cli = Cli::try_parse_from(&args).unwrap();
                match cli.command {
                    Command::Status(args) => args.selector(),
                    Command::Open(args) => args.selector(),
                    Command::Release(args) => args.selector(),
                    _ => unreachable!(),
                }
                .unwrap();
                for other in inputs.iter().skip(index + 1) {
                    let mut conflicting = args.clone();
                    conflicting.extend(other);
                    assert_eq!(
                        Cli::try_parse_from(conflicting).unwrap_err().kind(),
                        clap::error::ErrorKind::ArgumentConflict
                    );
                }
                if index > 0 {
                    args.extend(input);
                    assert_eq!(
                        Cli::try_parse_from(args).unwrap_err().kind(),
                        clap::error::ErrorKind::ArgumentConflict
                    );
                }
            }
            let missing = Cli::try_parse_from(["trees", command]);
            assert_eq!(missing.is_err(), command == "open");
            let invalid = Cli::try_parse_from(["trees", command, "--workspace-id", "invalid"]);
            assert_eq!(
                invalid.unwrap_err().kind(),
                clap::error::ErrorKind::ValueValidation
            );
            let invalid = Cli::try_parse_from(["trees", command, "--claim-id", "invalid"]).unwrap();
            let result = match invalid.command {
                Command::Status(args) => args.selector(),
                Command::Open(args) => args.selector(),
                Command::Release(args) => args.selector(),
                _ => unreachable!(),
            };
            assert!(matches!(result, Err(InputError::ClaimId { .. })));
            let mut parser = Cli::command();
            let help = parser
                .find_subcommand_mut(command)
                .unwrap()
                .render_long_help()
                .to_string();
            for option in ["--workspace-id", "--workspace-dir", "--claim-id"] {
                assert!(help.contains(option));
            }
        }
    }

    #[test]
    fn rejects_direct_conflicts_before_parsing_or_filesystem_access() {
        let args = WorkspaceLocatorArgs {
            workspace_dir: Some(PathBuf::from("/missing/parent/workspace")),
            claim_id: Some("invalid".to_owned()),
            ..Default::default()
        };
        assert!(matches!(
            args.resolve_with_directory(None, SelectionDefault::CurrentDirectory, || panic!(
                "unexpected directory access"
            )),
            Err(InputError::Conflict)
        ));
        let args = WorkspaceLocatorArgs {
            workspace_id: Some(WorkspaceId::new()),
            ..Default::default()
        };
        assert!(matches!(
            args.resolve(
                Some(WorkspaceLocatorInput::Id(args.workspace_id.unwrap())),
                SelectionDefault::CurrentDirectory
            ),
            Err(InputError::Conflict)
        ));
    }

    #[test]
    fn explicit_identifiers_do_not_read_the_directory() {
        let id = WorkspaceId::new();
        let claim = ClaimId::new();
        for args in [
            WorkspaceLocatorArgs {
                workspace_id: Some(id),
                ..Default::default()
            },
            WorkspaceLocatorArgs {
                claim_id: Some(claim.to_string()),
                ..Default::default()
            },
        ] {
            args.resolve_with_directory(None, SelectionDefault::CurrentDirectory, || {
                panic!("unexpected directory access")
            })
            .unwrap();
        }
    }

    #[test]
    fn applies_defaults_and_normalizes_relative_paths() {
        let args = WorkspaceLocatorArgs::default();
        assert!(matches!(
            args.resolve_with_directory(None, SelectionDefault::RequireExplicit, || panic!(
                "unexpected directory access"
            )),
            Err(InputError::Required)
        ));
        let expected = CanonicalPath::from_absolute("/work/project").unwrap();
        let selected = args
            .resolve_with_directory(None, SelectionDefault::CurrentDirectory, || {
                Ok(expected.clone())
            })
            .unwrap();
        assert!(
            matches!(selected, WorkspaceSelector::ContainingDirectory(path) if path == expected)
        );
        let selected = args
            .resolve(
                Some(WorkspaceLocatorInput::ExactPath(PathBuf::from("."))),
                SelectionDefault::RequireExplicit,
            )
            .unwrap();
        assert!(
            matches!(selected, WorkspaceSelector::ExactPath(path) if path == CanonicalPath::resolve(".").unwrap())
        );
    }
}
