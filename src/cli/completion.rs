use std::collections::BTreeSet;
use std::ffi::OsStr;

use clap::{Command, CommandFactory};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use diesel::result::QueryResult;
use diesel::sqlite::SqliteConnection;
use diesel::Connection;

use crate::database;
use crate::domain::WorkspaceState;
use crate::storage;

use super::Cli;

pub fn command() -> Command {
    let mut command = Cli::command();
    for binding in ID_BINDINGS {
        command = command.mut_subcommand(binding.command, |subcommand| {
            subcommand.mut_arg(binding.argument, |argument| {
                argument.add(ArgValueCompleter::new(move |current: &OsStr| {
                    complete_ids(binding.kind, current)
                }))
            })
        });
    }
    command
}

#[derive(Clone, Copy, Debug)]
enum CandidateKind {
    Workspace { include_removed: bool },
    Claim,
    Origin,
    WorkspaceOrOrigin,
}

#[derive(Clone, Copy, Debug)]
struct IdBinding {
    command: &'static str,
    argument: &'static str,
    kind: CandidateKind,
}

const ID_BINDINGS: &[IdBinding] = &[
    IdBinding {
        command: "add",
        argument: "workspace_id",
        kind: CandidateKind::Workspace {
            include_removed: false,
        },
    },
    IdBinding {
        command: "add",
        argument: "claim_id",
        kind: CandidateKind::Claim,
    },
    IdBinding {
        command: "claim",
        argument: "workspace_id",
        kind: CandidateKind::Workspace {
            include_removed: false,
        },
    },
    IdBinding {
        command: "release",
        argument: "workspace_id",
        kind: CandidateKind::Workspace {
            include_removed: false,
        },
    },
    IdBinding {
        command: "release",
        argument: "claim_id",
        kind: CandidateKind::Claim,
    },
    IdBinding {
        command: "status",
        argument: "legacy_workspace_id",
        kind: CandidateKind::Workspace {
            include_removed: true,
        },
    },
    IdBinding {
        command: "status",
        argument: "workspace_id",
        kind: CandidateKind::Workspace {
            include_removed: true,
        },
    },
    IdBinding {
        command: "status",
        argument: "claim_id",
        kind: CandidateKind::Claim,
    },
    IdBinding {
        command: "open",
        argument: "legacy_workspace_id",
        kind: CandidateKind::WorkspaceOrOrigin,
    },
    IdBinding {
        command: "open",
        argument: "workspace_id",
        kind: CandidateKind::Workspace {
            include_removed: false,
        },
    },
    IdBinding {
        command: "open",
        argument: "claim_id",
        kind: CandidateKind::Claim,
    },
    IdBinding {
        command: "remove",
        argument: "workspace_id",
        kind: CandidateKind::WorkspaceOrOrigin,
    },
];

fn complete_ids(kind: CandidateKind, current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(prefix) = current.to_str() else {
        return Vec::new();
    };
    let Ok(mut connection) = database::open_read_only() else {
        return Vec::new();
    };
    candidates(&mut connection, kind, prefix).unwrap_or_default()
}

fn candidates(
    connection: &mut SqliteConnection,
    kind: CandidateKind,
    prefix: &str,
) -> QueryResult<Vec<CompletionCandidate>> {
    connection.transaction(|connection| {
        let ids = load_ids(connection, kind)?;
        Ok(ids
            .into_iter()
            .filter(|id| id.starts_with(prefix))
            .map(CompletionCandidate::new)
            .collect())
    })
}

fn load_ids(
    connection: &mut SqliteConnection,
    kind: CandidateKind,
) -> QueryResult<BTreeSet<String>> {
    match kind {
        CandidateKind::Workspace { include_removed } => {
            Ok(storage::completion::workspaces(connection)?
                .into_iter()
                .filter(|(_, state)| include_removed || *state != WorkspaceState::Removed)
                .map(|(id, _)| id.to_string())
                .collect())
        }
        CandidateKind::Claim => Ok(storage::completion::claims(connection)?
            .into_iter()
            .map(|id| id.to_string())
            .collect()),
        CandidateKind::Origin => Ok(storage::completion::origins(connection)?
            .into_iter()
            .map(|id| id.to_string())
            .collect()),
        CandidateKind::WorkspaceOrOrigin => {
            let workspaces = storage::completion::workspaces(connection)?;
            let origins = load_ids(connection, CandidateKind::Origin)?;
            let workspace_ids: BTreeSet<String> =
                workspaces.iter().map(|(id, _)| id.to_string()).collect();
            Ok(workspaces
                .into_iter()
                .filter(|(_, state)| *state != WorkspaceState::Removed)
                .map(|(id, _)| id.to_string())
                .chain(origins.iter().cloned())
                .filter(|id| !(workspace_ids.contains(id) && origins.contains(id)))
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use clap::ValueHint;
    use clap_complete::engine::complete;

    use super::*;

    fn values(args: &[&str]) -> Vec<String> {
        let arg_index = args.len() - 1;
        let args = args.iter().map(OsString::from).collect();
        complete(&mut command(), args, arg_index, None)
            .expect("completion should succeed")
            .into_iter()
            .map(|candidate| candidate.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn completes_commands_options_and_finite_values() {
        assert!(values(&["trees", "st"]).contains(&"status".to_owned()));
        assert!(values(&["trees", "status", "--vi"]).contains(&"--view".to_owned()));
        assert!(values(&["trees", "status", "--view", "wor"]).contains(&"workspaces".to_owned()));
        assert!(values(&["trees", "config", "set", "orig"]).contains(&"origins-dir".to_owned()));
    }

    #[test]
    fn workspace_paths_have_directory_hints() {
        let cli = command();
        for (subcommand, argument) in [
            ("create", "workspace_path"),
            ("add", "add_workspace_dir"),
            ("claim", "workspace_dir"),
            ("release", "legacy_workspace_dir"),
            ("status", "workspace_dir"),
            ("open", "workspace_dir"),
        ] {
            let argument = cli
                .find_subcommand(subcommand)
                .unwrap()
                .get_arguments()
                .find(|item| item.get_id() == argument)
                .unwrap();
            assert_eq!(argument.get_value_hint(), ValueHint::DirPath);
        }
    }

    #[test]
    fn completes_a_workspace_directory() {
        let root = std::env::temp_dir().join(format!(
            "trees-completion-path-{}",
            crate::domain::WorkspaceId::new()
        ));
        fs::create_dir_all(root.join("example")).unwrap();

        let args = ["trees", "open", "--workspace-dir", "exa"]
            .into_iter()
            .map(OsString::from)
            .collect();
        let candidates = complete(&mut command(), args, 3, Some(&root)).unwrap();
        assert!(candidates.iter().any(|candidate| candidate
            .get_value()
            .to_string_lossy()
            .starts_with("example")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_id_argument_has_a_completion_binding() {
        let declared: BTreeSet<(String, String)> = Cli::command()
            .get_subcommands()
            .flat_map(|subcommand| {
                subcommand.get_arguments().filter_map(|argument| {
                    argument.get_value_names().and_then(|names| {
                        names
                            .iter()
                            .any(|name| {
                                let name = name.as_str();
                                name == "ID" || name.ends_with("_ID")
                            })
                            .then(|| {
                                (
                                    subcommand.get_name().to_owned(),
                                    argument.get_id().to_string(),
                                )
                            })
                    })
                })
            })
            .collect();
        let bound: BTreeSet<(String, String)> = ID_BINDINGS
            .iter()
            .map(|binding| (binding.command.to_owned(), binding.argument.to_owned()))
            .collect();
        assert_eq!(bound.len(), ID_BINDINGS.len());
        assert_eq!(declared, bound);

        let command = command();
        for binding in ID_BINDINGS {
            let argument = command
                .find_subcommand(binding.command)
                .unwrap()
                .get_arguments()
                .find(|argument| argument.get_id() == binding.argument)
                .unwrap();
            assert!(argument.get::<ArgValueCompleter>().is_some());
        }
    }
}
