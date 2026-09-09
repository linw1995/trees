use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use snafu::Snafu;

const WORKSPACE_CONTEXT_SEPARATOR: &str = "\n\n--- Trees workspace context ---\n";

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

pub fn merge_codex_args(
    args: &[OsString],
    cwd: &Path,
    managed_roots: &[PathBuf],
    developer_instructions: &str,
) -> Result<Vec<OsString>, CodexArgumentError> {
    let mut merged = Vec::with_capacity(args.len() + managed_roots.len() * 2 + 4);
    let mut has_cwd = false;
    let mut forwarded_roots = Vec::new();
    let mut forwarded_instructions = None;
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
            has_cwd = true;
            merged.push(args[index].clone());
            merged.push(value);
            index += 2;
            continue;
        }

        if argument.starts_with("--cd=") {
            if argument == "--cd=" {
                return Err(CodexArgumentError::EmptyValue {
                    option: "--cd".to_owned(),
                });
            }
            has_cwd = true;
            merged.push(args[index].clone());
            index += 1;
            continue;
        }

        if argument == "--add-dir" {
            let value =
                args.get(index + 1)
                    .cloned()
                    .ok_or_else(|| CodexArgumentError::MissingValue {
                        option: argument.clone(),
                    })?;
            forwarded_roots.push(PathBuf::from(&value));
            merged.push(args[index].clone());
            merged.push(value);
            index += 2;
            continue;
        }

        if let Some(value) = argument.strip_prefix("--add-dir=") {
            if value.is_empty() {
                return Err(CodexArgumentError::EmptyValue {
                    option: "--add-dir".to_owned(),
                });
            }
            forwarded_roots.push(PathBuf::from(value));
            merged.push(args[index].clone());
            index += 1;
            continue;
        }

        if argument == "-c" || argument == "--config" {
            let value = args.get(index + 1).cloned();
            if let Some(instructions) = value.as_ref().and_then(developer_instructions_value) {
                forwarded_instructions = Some(instructions);
                index += 2;
                continue;
            }
            merged.push(args[index].clone());
            if let Some(value) = value {
                merged.push(value);
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }

        if let Some(value) = argument
            .strip_prefix("--config=")
            .and_then(developer_instructions_value_text)
        {
            forwarded_instructions = Some(value);
            index += 1;
            continue;
        }

        merged.push(args[index].clone());
        index += 1;
    }

    if !has_cwd {
        merged.push(OsString::from("--cd"));
        merged.push(cwd.as_os_str().to_owned());
    }

    for root in managed_roots {
        if !forwarded_roots
            .iter()
            .any(|forwarded| equivalent_paths(forwarded, root, cwd))
        {
            merged.push(OsString::from("--add-dir"));
            merged.push(root.as_os_str().to_owned());
        }
    }

    let instructions = match forwarded_instructions {
        Some(forwarded) => merge_developer_instructions(&forwarded, developer_instructions),
        None => developer_instructions.to_owned(),
    };
    merged.push(OsString::from("--config"));
    merged.push(OsString::from(format!(
        "developer_instructions={}",
        serde_json::to_string(&instructions).expect("string serialization should not fail")
    )));

    Ok(merged)
}

fn developer_instructions_value(value: &OsString) -> Option<String> {
    developer_instructions_value_text(&value.to_string_lossy())
}

fn developer_instructions_value_text(value: &str) -> Option<String> {
    let (key, raw_value) = value.split_once('=')?;
    if key != "developer_instructions" {
        return None;
    }
    serde_json::from_str(raw_value)
        .ok()
        .or_else(|| Some(raw_value.to_owned()))
}

fn merge_developer_instructions(forwarded: &str, generated: &str) -> String {
    let manifest = generated
        .split_once(WORKSPACE_CONTEXT_SEPARATOR)
        .map(|(_, manifest)| manifest)
        .unwrap_or(generated);
    format!("{forwarded}{WORKSPACE_CONTEXT_SEPARATOR}{manifest}")
}

fn equivalent_paths(first: &Path, second: &Path, cwd: &Path) -> bool {
    normalize_path(first, cwd) == normalize_path(second, cwd)
}

fn normalize_path(path: &Path, cwd: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    fs::canonicalize(&absolute).unwrap_or(absolute)
}

#[derive(Debug, PartialEq, Eq, Snafu)]
pub enum CodexArgumentError {
    #[snafu(display("{option} requires a directory"))]
    MissingValue { option: String },
    #[snafu(display("{option} requires a non-empty directory"))]
    EmptyValue { option: String },
}

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

    #[test]
    fn merges_managed_roots_without_duplicate_add_dir_arguments() {
        let merged = merge_codex_args(
            &args(&["--model", "gpt-5.5", "--add-dir", "/workspace/one"]),
            Path::new("/workspace"),
            &[
                PathBuf::from("/workspace/one"),
                PathBuf::from("/workspace/two"),
            ],
            "Trees workspace context",
        )
        .expect("Codex arguments should merge");

        assert_eq!(
            merged,
            [
                OsString::from("--model"),
                OsString::from("gpt-5.5"),
                OsString::from("--add-dir"),
                OsString::from("/workspace/one"),
                OsString::from("--cd"),
                OsString::from("/workspace"),
                OsString::from("--add-dir"),
                OsString::from("/workspace/two"),
                OsString::from("--config"),
                OsString::from("developer_instructions=\"Trees workspace context\"")
            ]
        );
    }

    #[test]
    fn merges_forwarded_developer_instructions_before_the_workspace_manifest() {
        let merged = merge_codex_args(
            &args(&["-c", "developer_instructions=\"User context\""]),
            Path::new("/workspace"),
            &[],
            "User context\n\n--- Trees workspace context ---\nTrees manifest",
        )
        .expect("developer instructions should merge");
        let value = merged
            .windows(2)
            .find(|pair| pair[0] == "--config")
            .and_then(|pair| pair.get(1))
            .expect("merged config should be present")
            .to_string_lossy();

        assert_eq!(
            value,
            "developer_instructions=\"User context\\n\\n--- Trees workspace context ---\\nTrees manifest\""
        );
    }

    #[test]
    fn preserves_existing_cwd_and_add_dir_argument_forms() {
        let merged = merge_codex_args(
            &args(&[
                "--cd=/workspace",
                "--add-dir=/workspace/one",
                "--add-dir",
                "/workspace/two",
                "--config",
                "model=\"gpt-5.5\"",
            ]),
            Path::new("/fallback"),
            &[
                PathBuf::from("/workspace/one"),
                PathBuf::from("/workspace/two"),
            ],
            "Trees workspace context",
        )
        .expect("Codex arguments should merge");

        assert!(merged.contains(&OsString::from("--cd=/workspace")));
        assert!(merged.contains(&OsString::from("--add-dir=/workspace/one")));
        assert!(merged.contains(&OsString::from("--add-dir")));
        assert!(merged.contains(&OsString::from("model=\"gpt-5.5\"")));
        assert!(!merged.contains(&OsString::from("/fallback")));
    }

    #[test]
    fn rejects_missing_or_empty_add_dir_values() {
        assert_eq!(
            merge_codex_args(
                &args(&["--add-dir"]),
                Path::new("/workspace"),
                &[],
                "context"
            ),
            Err(CodexArgumentError::MissingValue {
                option: "--add-dir".to_owned()
            })
        );
        assert_eq!(
            merge_codex_args(
                &args(&["--add-dir="]),
                Path::new("/workspace"),
                &[],
                "context"
            ),
            Err(CodexArgumentError::EmptyValue {
                option: "--add-dir".to_owned()
            })
        );
    }

    #[test]
    fn preserves_non_developer_config_and_missing_config_values() {
        let merged = merge_codex_args(
            &args(&["--config", "model=\"gpt-5.5\"", "-c"]),
            Path::new("/workspace"),
            &[],
            "context",
        )
        .expect("non-developer config should be preserved");

        assert!(merged
            .windows(2)
            .any(|pair| { pair[0] == "--config" && pair[1] == "model=\"gpt-5.5\"" }));
        assert!(merged.contains(&OsString::from("-c")));
    }

    #[test]
    fn parses_equals_form_developer_instructions() {
        let merged = merge_codex_args(
            &args(&["--config=developer_instructions=\"User context\""]),
            Path::new("/workspace"),
            &[],
            "Trees workspace context",
        )
        .expect("developer instructions should merge");

        assert!(merged.iter().any(|argument| {
            argument
                .to_string_lossy()
                .contains("developer_instructions=\"User context")
        }));
    }
}
