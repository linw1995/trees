use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::paths;
use snafu::Snafu;

const WORKSPACES_DIR_KEY: &str = "workspaces_dir";

pub fn workspaces_directory() -> Result<PathBuf, ConfigError> {
    let configuration_path =
        paths::configuration_path().map_err(|source| ConfigError::Path { source })?;
    let table = load_table(&configuration_path)?;
    let default = paths::default_managed_workspace_directory()
        .map_err(|source| ConfigError::Path { source })?;
    let default = normalize_existing_path(&configuration_path, default)?;
    configured_workspaces_directory(&configuration_path, &table, default)
}

pub fn set_workspaces_directory(path: &Path) -> Result<PathBuf, ConfigError> {
    let configuration_path =
        paths::configuration_path().map_err(|source| ConfigError::Path { source })?;
    set_workspaces_directory_at(&configuration_path, path)
}

fn configured_workspaces_directory(
    configuration_path: &Path,
    table: &toml::Table,
    default: PathBuf,
) -> Result<PathBuf, ConfigError> {
    let Some(workspace) = table.get("workspace") else {
        return Ok(default);
    };
    let workspace = workspace.as_table().ok_or_else(|| ConfigError::Invalid {
        path: configuration_path.to_owned(),
        reason: "workspace must be a TOML table".to_owned(),
    })?;
    let Some(value) = workspace.get(WORKSPACES_DIR_KEY) else {
        return Ok(default);
    };
    let value = value.as_str().ok_or_else(|| ConfigError::Invalid {
        path: configuration_path.to_owned(),
        reason: "workspace.workspaces_dir must be a string".to_owned(),
    })?;
    normalize_existing_path(
        configuration_path,
        resolve_configured_path(configuration_path, Path::new(value)),
    )
}

fn set_workspaces_directory_at(
    configuration_path: &Path,
    path: &Path,
) -> Result<PathBuf, ConfigError> {
    if path.as_os_str().is_empty() {
        return Err(ConfigError::Invalid {
            path: configuration_path.to_owned(),
            reason: "workspace directory must not be empty".to_owned(),
        });
    }
    let mut table = load_table(configuration_path)?;
    let resolved = resolve_configured_path(configuration_path, path);
    let workspace = table
        .entry("workspace".to_owned())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let workspace = workspace
        .as_table_mut()
        .ok_or_else(|| ConfigError::Invalid {
            path: configuration_path.to_owned(),
            reason: "workspace must be a TOML table".to_owned(),
        })?;
    workspace.insert(
        WORKSPACES_DIR_KEY.to_owned(),
        toml::Value::String(resolved.to_string_lossy().into_owned()),
    );
    let parent = configuration_path
        .parent()
        .ok_or_else(|| ConfigError::Invalid {
            path: configuration_path.to_owned(),
            reason: "configuration path has no parent directory".to_owned(),
        })?;
    fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
        path: parent.to_owned(),
        source,
    })?;
    let document = toml::to_string_pretty(&toml::Value::Table(table)).map_err(|source| {
        ConfigError::Serialize {
            path: configuration_path.to_owned(),
            source,
        }
    })?;
    fs::write(configuration_path, document).map_err(|source| ConfigError::Io {
        path: configuration_path.to_owned(),
        source,
    })?;
    Ok(resolved)
}

fn load_table(path: &Path) -> Result<toml::Table, ConfigError> {
    let document = match fs::read_to_string(path) {
        Ok(document) => document,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(toml::Table::new());
        }
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_owned(),
                source,
            });
        }
    };
    let value = toml::from_str::<toml::Value>(&document).map_err(|source| ConfigError::Parse {
        path: path.to_owned(),
        source,
    })?;
    value
        .as_table()
        .cloned()
        .ok_or_else(|| ConfigError::Invalid {
            path: path.to_owned(),
            reason: "configuration root must be a TOML table".to_owned(),
        })
}

fn resolve_configured_path(configuration_path: &Path, path: &Path) -> PathBuf {
    let base = configuration_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    normalize_path(if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    })
}

fn normalize_existing_path(
    configuration_path: &Path,
    path: PathBuf,
) -> Result<PathBuf, ConfigError> {
    if path.exists() {
        fs::canonicalize(&path).map_err(|source| ConfigError::Io {
            path: configuration_path.to_owned(),
            source,
        })
    } else {
        Ok(path)
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

#[derive(Debug, Snafu)]
pub enum ConfigError {
    #[snafu(display("failed to resolve configuration path: {source}"))]
    Path { source: paths::PathError },
    #[snafu(display("configuration filesystem operation failed for {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to parse configuration {}: {source}", path.display()))]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[snafu(display("failed to serialize configuration {}: {source}", path.display()))]
    Serialize {
        path: PathBuf,
        source: toml::ser::Error,
    },
    #[snafu(display("invalid configuration {}: {reason}", path.display()))]
    Invalid { path: PathBuf, reason: String },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-config-{}", uuid::Uuid::now_v7()))
    }

    #[test]
    fn resolves_relative_workspace_directories_from_the_configuration_directory() {
        let root = test_root();
        let configuration_path = root.join("config.toml");

        let resolved =
            set_workspaces_directory_at(&configuration_path, Path::new("../managed-workspaces"))
                .expect("configuration should be written");

        assert!(resolved.is_absolute());
        assert_eq!(resolved, normalize_path(root.join("../managed-workspaces")));
        let table = load_table(&configuration_path).expect("configuration should be readable");
        assert_eq!(
            configured_workspaces_directory(
                &configuration_path,
                &table,
                PathBuf::from("/default/workspaces"),
            )
            .expect("workspace directory should resolve"),
            resolved
        );

        fs::remove_dir_all(root).expect("configuration test root should be removable");
    }

    #[test]
    fn preserves_unrelated_configuration_values_when_setting_workspace_directory() {
        let root = test_root();
        let configuration_path = root.join("config.toml");
        fs::create_dir_all(&root).expect("configuration directory should be created");
        fs::write(&configuration_path, "[other]\nvalue = \"keep\"\n")
            .expect("configuration should be written");

        set_workspaces_directory_at(&configuration_path, Path::new("workspaces"))
            .expect("workspace directory should be configured");
        let table = load_table(&configuration_path).expect("configuration should be readable");
        assert_eq!(
            table
                .get("other")
                .and_then(toml::Value::as_table)
                .and_then(|table| table.get("value"))
                .and_then(toml::Value::as_str),
            Some("keep")
        );

        fs::remove_dir_all(root).expect("configuration test root should be removable");
    }

    #[test]
    fn formats_configuration_errors() {
        let path = PathBuf::from("/tmp/config.toml");
        let parse_source = toml::from_str::<toml::Value>("[").expect_err("TOML should be invalid");
        let serialize_source = toml::to_string(&f64::NAN).expect_err("NaN should not serialize");
        let errors = [
            ConfigError::Path {
                source: paths::PathError::HomeDirectoryUnavailable,
            },
            ConfigError::Io {
                path: path.clone(),
                source: std::io::Error::other("read failed"),
            },
            ConfigError::Parse {
                path: path.clone(),
                source: parse_source,
            },
            ConfigError::Serialize {
                path: path.clone(),
                source: serialize_source,
            },
            ConfigError::Invalid {
                path,
                reason: "invalid value".to_owned(),
            },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }
}
