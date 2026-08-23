use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

pub fn workspace_path_from_codex_args(args: &[OsString]) -> Result<PathBuf, CodexArgumentError> {
    let mut workspace_path = None;
    let mut index = 0;

    while index < args.len() {
        let argument = args[index].to_string_lossy().into_owned();
        if argument == "-C" || argument == "--cd" {
            let value =
                args.get(index + 1)
                    .cloned()
                    .ok_or_else(|| CodexArgumentError::MissingValue {
                        option: argument.clone(),
                    })?;
            if value.is_empty() {
                return Err(CodexArgumentError::EmptyValue {
                    option: argument.clone(),
                });
            }
            workspace_path = Some(PathBuf::from(value));
            index += 2;
            continue;
        }

        if let Some(value) = argument.strip_prefix("--cd=") {
            if value.is_empty() {
                return Err(CodexArgumentError::EmptyValue {
                    option: "--cd".to_owned(),
                });
            }
            workspace_path = Some(PathBuf::from(value));
            index += 1;
            continue;
        }

        index += 1;
    }

    Ok(workspace_path.unwrap_or_else(|| PathBuf::from(".")))
}

#[derive(Debug, PartialEq, Eq)]
pub enum CodexArgumentError {
    MissingValue { option: String },
    EmptyValue { option: String },
}

impl fmt::Display for CodexArgumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue { option } => write!(formatter, "{option} requires a directory"),
            Self::EmptyValue { option } => {
                write!(formatter, "{option} requires a non-empty directory")
            }
        }
    }
}

impl std::error::Error for CodexArgumentError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn defaults_to_the_current_directory() {
        assert_eq!(
            workspace_path_from_codex_args(&[]).expect("default path should resolve"),
            PathBuf::from(".")
        );
    }

    #[test]
    fn reads_separated_short_and_long_cd_values() {
        assert_eq!(
            workspace_path_from_codex_args(&args(&["-C", "/one"]))
                .expect("short cd should resolve"),
            PathBuf::from("/one")
        );
        assert_eq!(
            workspace_path_from_codex_args(&args(&["--cd", "/two"]))
                .expect("long cd should resolve"),
            PathBuf::from("/two")
        );
    }

    #[test]
    fn reads_equals_form_and_uses_the_last_value() {
        assert_eq!(
            workspace_path_from_codex_args(&args(&[
                "--cd=/one",
                "--model",
                "gpt-5.5",
                "-C",
                "/two",
            ]))
            .expect("last cd should resolve"),
            PathBuf::from("/two")
        );
    }

    #[test]
    fn rejects_missing_or_empty_cd_values() {
        assert_eq!(
            workspace_path_from_codex_args(&args(&["--cd"])),
            Err(CodexArgumentError::MissingValue {
                option: "--cd".to_owned()
            })
        );
        assert_eq!(
            workspace_path_from_codex_args(&args(&["--cd="])),
            Err(CodexArgumentError::EmptyValue {
                option: "--cd".to_owned()
            })
        );
    }
}
