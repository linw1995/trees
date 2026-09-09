use std::path::{Path, PathBuf};

use snafu::{ensure, Snafu};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryInput {
    Path(PathBuf),
    Url(String),
    Name(String),
}

impl RepositoryInput {
    pub fn parse(value: &Path) -> Result<Self, InputError> {
        let Some(text) = value.to_str() else {
            return Ok(Self::Path(value.to_owned()));
        };
        ensure!(!text.is_empty(), EmptySnafu);
        if explicit_path(text) || value.is_absolute() {
            return Ok(Self::Path(value.to_owned()));
        }
        if text.contains("://") || scp_remote(text) {
            validate_url(text)?;
            return Ok(Self::Url(text.to_owned()));
        }
        if text.contains('/') || text.contains('\\') || value.exists() {
            return Ok(Self::Path(value.to_owned()));
        }
        Ok(Self::Name(text.to_owned()))
    }
}

fn explicit_path(value: &str) -> bool {
    value == "."
        || value == ".."
        || value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with(".\\")
        || value.starts_with("..\\")
        || (value.as_bytes().get(1) == Some(&b':') && value.as_bytes()[0].is_ascii_alphabetic())
}

fn scp_remote(value: &str) -> bool {
    value.split_once(':').is_some_and(|(host, path)| {
        !host.is_empty() && !host.contains(['/', '\\']) && !path.is_empty()
    })
}

fn validate_url(value: &str) -> Result<(), InputError> {
    ensure!(
        !value.starts_with('-') && !value.chars().any(char::is_control),
        InvalidUrlSnafu
    );
    if let Some((scheme, rest)) = value.split_once("://") {
        ensure!(
            ["https", "http", "ssh", "git", "file"].contains(&scheme) && !rest.is_empty(),
            InvalidUrlSnafu
        );
        let authority = rest.split('/').next().unwrap_or_default();
        let has_secret = match scheme {
            "http" | "https" => authority.contains('@') || rest.contains(['?', '#']),
            _ => authority
                .split_once('@')
                .is_some_and(|(userinfo, _)| userinfo.contains(':')),
        };
        ensure!(!has_secret, CredentialUrlSnafu);
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum InputError {
    #[snafu(display("repository input must not be empty"))]
    Empty,
    #[snafu(display("unsupported or invalid repository URL"))]
    InvalidUrl,
    #[snafu(display("repository URLs must not contain embedded credentials; use Git authentication configuration"))]
    CredentialUrl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_urls_names_and_explicit_paths() {
        for path in [
            "./api",
            "../api",
            "/tmp/api",
            "C:\\repos\\api",
            "./host:api",
        ] {
            assert_eq!(
                RepositoryInput::parse(Path::new(path)).unwrap(),
                RepositoryInput::Path(path.into())
            );
        }
        for url in [
            "https://example.com/api.git",
            "ssh://git@example.com/api",
            "git@example.com:api.git",
            "file:///tmp/api",
        ] {
            assert_eq!(
                RepositoryInput::parse(Path::new(url)).unwrap(),
                RepositoryInput::Url(url.into())
            );
        }
        assert_eq!(
            RepositoryInput::parse(Path::new("unlikely-registered-repository-name")).unwrap(),
            RepositoryInput::Name("unlikely-registered-repository-name".into())
        );
        assert!(matches!(
            RepositoryInput::parse(Path::new("src")).unwrap(),
            RepositoryInput::Path(_)
        ));
    }

    #[test]
    fn rejects_unsafe_urls_without_echoing_credentials() {
        for url in [
            "https://secret@example.com/api",
            "ext://command",
            "https://host/api?token=secret",
            "ssh://user:secret@host/api",
            "https://host/\napi",
        ] {
            let error = RepositoryInput::parse(Path::new(url)).unwrap_err();
            assert!(!error.to_string().contains("secret"));
        }
    }
}
