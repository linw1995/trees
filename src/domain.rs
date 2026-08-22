use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use diesel::{AsExpression, FromSqlRow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

macro_rules! uuid_identifier {
    ($name:ident) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            Eq,
            PartialEq,
            Hash,
            Serialize,
            Deserialize,
            AsExpression,
            FromSqlRow,
        )]
        #[diesel(sql_type = diesel::sql_types::Text)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            pub fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::try_from(value.to_owned())
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdentifierError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                let uuid = Uuid::parse_str(&value).map_err(IdentifierError::InvalidUuid)?;
                if uuid.get_version_num() != 7 {
                    return Err(IdentifierError::NotUuidV7 { value });
                }

                Ok(Self(uuid))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.to_string()
            }
        }
    };
}

uuid_identifier!(WorkspaceId);
uuid_identifier!(RepoWorktreeId);
uuid_identifier!(OperationId);
uuid_identifier!(EventId);

#[derive(Debug)]
pub enum IdentifierError {
    InvalidUuid(uuid::Error),
    NotUuidV7 { value: String },
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUuid(error) => write!(formatter, "invalid UUID: {error}"),
            Self::NotUuidV7 { value } => write!(formatter, "UUID is not version 7: {value}"),
        }
    }
}

impl std::error::Error for IdentifierError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUuid(error) => Some(error),
            Self::NotUuidV7 { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Creating,
    Ready,
    Degraded,
    Failed,
}

impl WorkspaceState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for WorkspaceState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for WorkspaceState {
    type Err = StateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "creating" => Ok(Self::Creating),
            "ready" => Ok(Self::Ready),
            "degraded" => Ok(Self::Degraded),
            "failed" => Ok(Self::Failed),
            _ => Err(StateParseError::new(value, Self::ALL)),
        }
    }
}

impl WorkspaceState {
    const ALL: &'static [&'static str] = &["creating", "ready", "degraded", "failed"];
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(rename_all = "snake_case")]
pub enum RepoWorktreeState {
    Pending,
    Attached,
    Missing,
    Diverged,
    Failed,
}

impl RepoWorktreeState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Attached => "attached",
            Self::Missing => "missing",
            Self::Diverged => "diverged",
            Self::Failed => "failed",
        }
    }

    const ALL: &'static [&'static str] = &["pending", "attached", "missing", "diverged", "failed"];
}

impl fmt::Display for RepoWorktreeState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RepoWorktreeState {
    type Err = StateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "attached" => Ok(Self::Attached),
            "missing" => Ok(Self::Missing),
            "diverged" => Ok(Self::Diverged),
            "failed" => Ok(Self::Failed),
            _ => Err(StateParseError::new(value, Self::ALL)),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Running,
    Succeeded,
    Failed,
    RolledBack,
}

impl OperationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::RolledBack => "rolled_back",
        }
    }

    const ALL: &'static [&'static str] = &["running", "succeeded", "failed", "rolled_back"];
}

impl fmt::Display for OperationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for OperationState {
    type Err = StateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "rolled_back" => Ok(Self::RolledBack),
            _ => Err(StateParseError::new(value, Self::ALL)),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StateParseError {
    value: String,
    expected: &'static [&'static str],
}

impl StateParseError {
    fn new(value: &str, expected: &'static [&'static str]) -> Self {
        Self {
            value: value.to_owned(),
            expected,
        }
    }
}

impl fmt::Display for StateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid lifecycle state {:?}; expected one of {:?}",
            self.value, self.expected
        )
    }
}

impl std::error::Error for StateParseError {}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(transparent)]
pub struct CanonicalPath(PathBuf);

impl CanonicalPath {
    pub fn resolve(path: impl AsRef<Path>) -> Result<Self, CanonicalPathError> {
        let path = path.as_ref().to_owned();
        std::fs::canonicalize(&path)
            .map(Self)
            .map_err(|source| CanonicalPathError::Io { path, source })
    }

    pub fn from_absolute(path: impl AsRef<Path>) -> Result<Self, CanonicalPathError> {
        let path = path.as_ref().to_owned();
        if path.is_absolute() {
            Ok(Self(path))
        } else {
            Err(CanonicalPathError::NotAbsolute { path })
        }
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for CanonicalPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl fmt::Display for CanonicalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(formatter)
    }
}

impl FromStr for CanonicalPath {
    type Err = CanonicalPathError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_absolute(value)
    }
}

#[derive(Debug)]
pub enum CanonicalPathError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    NotAbsolute {
        path: PathBuf,
    },
}

impl fmt::Display for CanonicalPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "failed to canonicalize {}: {}",
                    path.display(),
                    source
                )
            }
            Self::NotAbsolute { path } => {
                write!(formatter, "path is not absolute: {}", path.display())
            }
        }
    }
}

impl std::error::Error for CanonicalPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NotAbsolute { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub struct JsonDocument(Value);

impl JsonDocument {
    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self, JsonDocumentError> {
        serde_json::to_value(value)
            .map(Self)
            .map_err(JsonDocumentError::Serialize)
    }

    pub fn parse(text: &str) -> Result<Self, JsonDocumentError> {
        serde_json::from_str(text)
            .map(Self)
            .map_err(JsonDocumentError::Parse)
    }

    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn into_value(self) -> Value {
        self.0
    }

    pub fn to_canonical_string(&self) -> Result<String, JsonDocumentError> {
        serde_json::to_string(&self.0).map_err(JsonDocumentError::Serialize)
    }
}

impl fmt::Display for JsonDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.to_string())
    }
}

impl FromStr for JsonDocument {
    type Err = JsonDocumentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

#[derive(Debug)]
pub enum JsonDocumentError {
    Parse(serde_json::Error),
    Serialize(serde_json::Error),
}

impl fmt::Display for JsonDocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "invalid JSON: {error}"),
            Self::Serialize(error) => write!(formatter, "failed to serialize JSON: {error}"),
        }
    }
}

impl std::error::Error for JsonDocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) | Self::Serialize(error) => Some(error),
        }
    }
}

#[derive(
    Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, AsExpression, FromSqlRow,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(transparent)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn now() -> Self {
        Self::from_datetime(OffsetDateTime::now_utc())
    }

    pub fn after_seconds(seconds: i64) -> Self {
        Self::from_datetime(OffsetDateTime::now_utc() + time::Duration::seconds(seconds))
    }

    fn from_datetime(value: OffsetDateTime) -> Self {
        let value = value
            .format(&Rfc3339)
            .expect("RFC 3339 formatting should be infallible");
        Self(value)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, TimestampError> {
        let value = value.into();
        OffsetDateTime::parse(&value, &Rfc3339)
            .map(|_| Self(value))
            .map_err(TimestampError)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub struct TimestampError(time::error::Parse);

impl fmt::Display for TimestampError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid RFC 3339 timestamp: {}", self.0)
    }
}

impl std::error::Error for TimestampError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_uuid_v7_and_round_trip_as_text() {
        let identifier = WorkspaceId::new();
        let text = identifier.to_string();

        assert_eq!(identifier.as_uuid().get_version_num(), 7);
        assert_eq!(
            WorkspaceId::from_str(&text).expect("identifier should parse"),
            identifier
        );
        assert!(WorkspaceId::from_str(&Uuid::nil().to_string()).is_err());
    }

    #[test]
    fn states_use_storage_names() {
        assert_eq!(WorkspaceState::Ready.to_string(), "ready");
        assert_eq!(
            RepoWorktreeState::from_str("diverged").unwrap(),
            RepoWorktreeState::Diverged
        );
        assert_eq!(
            OperationState::from_str("rolled_back").unwrap(),
            OperationState::RolledBack
        );
        assert!(OperationState::from_str("unknown").is_err());
    }

    #[test]
    fn canonical_path_resolves_existing_paths() {
        let path = CanonicalPath::resolve("Cargo.toml").expect("Cargo.toml should exist");

        assert!(path.as_path().is_absolute());
    }

    #[test]
    fn json_documents_are_validated_and_canonicalized() {
        let document = JsonDocument::parse(r#"{"b":2,"a":1}"#).expect("JSON should parse");

        assert_eq!(document.to_canonical_string().unwrap(), r#"{"a":1,"b":2}"#);
        assert!(JsonDocument::parse("not json").is_err());
    }

    #[test]
    fn timestamps_are_rfc3339() {
        let timestamp = Timestamp::now();

        assert_eq!(Timestamp::parse(timestamp.to_string()).unwrap(), timestamp);
        assert!(Timestamp::parse("not a timestamp").is_err());
    }
}
