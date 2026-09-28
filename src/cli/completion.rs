use clap::{Command, CommandFactory};

use super::Cli;

pub fn command() -> Command {
    Cli::command()
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
}
