mod runner;

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use snafu::{ensure, ResultExt, Snafu};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::WorkspaceStatus;
use crate::domain::Timestamp;

#[derive(Debug, Serialize)]
pub struct Request {
    pub version: u8,
    pub workspaces: Vec<RequestedWorkspace>,
}

#[derive(Debug, Serialize)]
pub struct RequestedWorkspace {
    pub id: String,
    pub path: String,
}

impl Request {
    pub fn new(workspaces: &[WorkspaceStatus]) -> Self {
        Self {
            version: 1,
            workspaces: workspaces
                .iter()
                .map(|workspace| RequestedWorkspace {
                    id: workspace.workspace_id.to_string(),
                    path: workspace.path.to_string(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Session {
    pub agent: String,
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
struct Response {
    version: u8,
    workspaces: BTreeMap<String, Sessions>,
}

#[derive(Debug, Deserialize)]
struct Sessions {
    sessions: Vec<Session>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub status: Completeness,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Serialize)]
pub struct Observation {
    pub observed_at: Timestamp,
    pub status: Completeness,
    pub workspaces: BTreeMap<String, Entry>,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    ConfigurationFailed,
    ExitFailed,
    InvalidResponse,
    IoFailed,
    MissingResult,
    OutputLimitExceeded,
    SpawnFailed,
    TimedOut,
}

impl IssueCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfigurationFailed => "configuration_failed",
            Self::ExitFailed => "exit_failed",
            Self::InvalidResponse => "invalid_response",
            Self::IoFailed => "io_failed",
            Self::MissingResult => "missing_result",
            Self::OutputLimitExceeded => "output_limit_exceeded",
            Self::SpawnFailed => "spawn_failed",
            Self::TimedOut => "timed_out",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub workspace_id: Option<String>,
    pub exit_code: Option<i32>,
    pub stderr: Option<String>,
}

impl Issue {
    pub fn new(code: IssueCode) -> Self {
        Self {
            code,
            workspace_id: None,
            exit_code: None,
            stderr: None,
        }
    }
}

impl Observation {
    pub fn unavailable(request: &Request, observed_at: Timestamp, issue: Issue) -> Self {
        Self {
            observed_at,
            status: Completeness::Unavailable,
            workspaces: request
                .workspaces
                .iter()
                .map(|workspace| {
                    (
                        workspace.id.clone(),
                        Entry {
                            status: Completeness::Unavailable,
                            sessions: Vec::new(),
                        },
                    )
                })
                .collect(),
            issues: vec![issue],
        }
    }
}

pub fn observe(workspaces: &[WorkspaceStatus], enabled: bool) -> Option<Observation> {
    let observed_at = Timestamp::now();
    let config = crate::config::session_hook(enabled);
    let request = Request::new(workspaces);
    match config {
        Ok(Some(config)) => Some(query(&config, &request)),
        Ok(None) => None,
        Err(_) => Some(Observation::unavailable(
            &request,
            observed_at,
            Issue::new(IssueCode::ConfigurationFailed),
        )),
    }
}

pub fn cell(observation: &Observation, workspace_id: &str) -> String {
    let Some(entry) = observation.workspaces.get(workspace_id) else {
        return "unavailable".into();
    };
    if entry.status != Completeness::Complete {
        return "unavailable".into();
    }
    let Some(session) = entry.sessions.first() else {
        return "—".into();
    };
    let label = format!("{}: {}", session.agent, session.title);
    let mut tokens = Vec::new();
    let mut width = 0;
    for character in label.chars() {
        let token = match character {
            '\\' => "\\\\".to_owned(),
            character if character.is_control() => character.escape_default().to_string(),
            character => character.to_string(),
        };
        let token_width = super::display_width(&token);
        if width + token_width > 60 {
            while width > 59 {
                let removed: String = tokens.pop().expect("nonempty label");
                width -= super::display_width(&removed);
            }
            tokens.push("…".to_owned());
            break;
        }
        width += token_width;
        tokens.push(token);
    }
    tokens.concat()
}

pub fn query(config: &crate::config::SessionHookConfig, request: &Request) -> Observation {
    let observed_at = Timestamp::now();
    match runner::execute(config, request) {
        Ok(bytes) => decode(request, observed_at.clone(), &bytes).unwrap_or_else(|_| {
            Observation::unavailable(request, observed_at, Issue::new(IssueCode::InvalidResponse))
        }),
        Err(issue) => Observation::unavailable(request, observed_at, issue),
    }
}

pub fn decode(
    request: &Request,
    observed_at: Timestamp,
    bytes: &[u8],
) -> Result<Observation, ProtocolError> {
    let value: UniqueValue = serde_json::from_slice(bytes).context(JsonSnafu)?;
    let response: Response = serde_json::from_value(value.0).context(JsonSnafu)?;
    ensure!(response.version == 1, InvalidSnafu);
    for (id, entry) in &response.workspaces {
        ensure!(
            request
                .workspaces
                .iter()
                .any(|workspace| &workspace.id == id),
            InvalidSnafu
        );
        for session in &entry.sessions {
            ensure!(
                !session.agent.trim().is_empty()
                    && !session.id.trim().is_empty()
                    && !session.title.trim().is_empty(),
                InvalidSnafu
            );
            OffsetDateTime::parse(&session.updated_at, &Rfc3339).context(TimestampSnafu)?;
        }
    }
    let mut response = response.workspaces;
    let mut issues = Vec::new();
    let workspaces = request
        .workspaces
        .iter()
        .map(|workspace| {
            let entry = match response.remove(&workspace.id) {
                Some(entry) => Entry {
                    status: Completeness::Complete,
                    sessions: entry.sessions,
                },
                None => {
                    issues.push(Issue {
                        workspace_id: Some(workspace.id.clone()),
                        ..Issue::new(IssueCode::MissingResult)
                    });
                    Entry {
                        status: Completeness::Unavailable,
                        sessions: Vec::new(),
                    }
                }
            };
            (workspace.id.clone(), entry)
        })
        .collect();
    issues.sort_by(|a, b| a.workspace_id.cmp(&b.workspace_id));
    let status = if issues.is_empty() {
        Completeness::Complete
    } else if issues.len() == request.workspaces.len() {
        Completeness::Unavailable
    } else {
        Completeness::Partial
    };
    Ok(Observation {
        observed_at,
        status,
        workspaces,
        issues,
    })
}

#[derive(Debug, Snafu)]
pub enum ProtocolError {
    #[snafu(display("invalid session response JSON: {source}"))]
    Json { source: serde_json::Error },
    #[snafu(display("invalid session response version, workspace, or session fields"))]
    Invalid,
    #[snafu(display("invalid session timestamp: {source}"))]
    Timestamp { source: time::error::Parse },
}

// Validate duplicate keys before decoding typed fields, including extension objects.
struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("JSON without duplicate keys")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut result = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
                    if result.insert(key, value.0).is_some() {
                        return Err(serde::de::Error::custom("duplicate object key"));
                    }
                }
                Ok(UniqueValue(Value::Object(result)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut result = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    result.push(value.0);
                }
                Ok(UniqueValue(Value::Array(result)))
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> Request {
        Request {
            version: 1,
            workspaces: ["a", "b", "c"]
                .into_iter()
                .map(|id| RequestedWorkspace {
                    id: id.into(),
                    path: format!("/work/{id}"),
                })
                .collect(),
        }
    }

    fn session(id: &str) -> Value {
        json!({"agent":"agent", "id":id, "title":"A title", "updated_at":"2026-09-17T00:00:00Z"})
    }

    #[test]
    fn preserves_lists_and_distinguishes_empty_from_missing() {
        let response = json!({"version":1,"workspaces":{"a":{"sessions":[session("first"),session("second")]},"b":{"sessions":[]}},"extension":true});
        let observed_at = Timestamp::now();
        let observation = decode(
            &request(),
            observed_at.clone(),
            &serde_json::to_vec(&response).unwrap(),
        )
        .unwrap();
        assert_eq!(observation.status, Completeness::Partial);
        assert_eq!(observation.observed_at, observed_at);
        assert_eq!(observation.workspaces["a"].sessions[0].id, "first");
        assert_eq!(observation.workspaces["a"].sessions[1].id, "second");
        assert_eq!(observation.workspaces["b"].status, Completeness::Complete);
        assert_eq!(
            observation.workspaces["c"].status,
            Completeness::Unavailable
        );
        assert_eq!(observation.issues[0].workspace_id.as_deref(), Some("c"));
        let json = serde_json::to_value(observation).unwrap();
        assert_eq!(
            json["workspaces"]["a"]["sessions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let empty = decode(
            &request(),
            Timestamp::now(),
            br#"{"version":1,"workspaces":{}}"#,
        )
        .unwrap();
        assert_eq!(empty.status, Completeness::Unavailable);
        let complete = decode(
            &request(),
            Timestamp::now(),
            br#"{"version":1,"workspaces":{"a":{"sessions":[]},"b":{"sessions":[]},"c":{"sessions":[]}},"extension":{"signed":-1,"fraction":0.5}}"#,
        )
        .unwrap();
        assert_eq!(complete.status, Completeness::Complete);
        assert!(complete.issues.is_empty());
        assert!(complete
            .workspaces
            .values()
            .all(|entry| { entry.status == Completeness::Complete && entry.sessions.is_empty() }));
    }

    #[test]
    fn renders_first_session_with_safe_width_and_complete_escapes() {
        let title = format!("{}\n\u{1b}[31m\\", "界".repeat(40));
        let mut first = session("first");
        first["title"] = json!(title);
        let response = json!({"version":1,"workspaces":{"a":{"sessions":[first,session("second")]},"b":{"sessions":[]}}});
        let observation = decode(
            &request(),
            Timestamp::now(),
            &serde_json::to_vec(&response).unwrap(),
        )
        .unwrap();
        let rendered = cell(&observation, "a");
        assert!(super::super::display_width(&rendered) <= 60);
        assert!(rendered.ends_with('…'));
        assert!(!rendered.contains('\n'));
        assert!(!rendered.contains('\u{1b}'));
        assert_eq!(cell(&observation, "b"), "—");
        assert_eq!(cell(&observation, "c"), "unavailable");
        assert_eq!(observation.workspaces["a"].sessions[0].title, title);
        let boundary = observation_for_title(&format!("{}\u{1b}x", "x".repeat(47)));
        assert_eq!(cell(&boundary, "a"), format!("agent: {}…", "x".repeat(47)));
        let boundary = observation_for_title(&format!("{}x", "x".repeat(53)));
        assert_eq!(cell(&boundary, "a"), format!("agent: {}…", "x".repeat(52)));
        for title in ["\n\u{1b}\\".to_owned(), format!("{}\u{1b}", "x".repeat(51))] {
            let mut observation = observation_for_title(&title);
            let rendered = cell(&observation, "a");
            assert!(!rendered.contains('\n'));
            assert!(!rendered.contains('\u{1b}'));
            assert!(!rendered.ends_with("\\u{…"));
            observation.workspaces.get_mut("a").unwrap().sessions[0].title = "x".repeat(53);
            assert_eq!(super::super::display_width(&cell(&observation, "a")), 60);
        }
    }

    fn observation_for_title(title: &str) -> Observation {
        let mut item = session("first");
        item["title"] = json!(title);
        decode(
            &request(),
            Timestamp::now(),
            &serde_json::to_vec(&json!({"version":1,"workspaces":{"a":{"sessions":[item]}}}))
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn rejects_invalid_responses_including_later_entries() {
        let mut invalid_time = session("bad");
        invalid_time["updated_at"] = json!("yesterday");
        for response in [
            json!({"version":2,"workspaces":{}}),
            json!({"version":1,"workspaces":{"unknown":{"sessions":[]}}}),
            json!({"version":1,"workspaces":{"a":{"sessions":null}}}),
            json!({"version":1,"workspaces":{"a":{"sessions":[null]}}}),
            json!({"version":1,"workspaces":{"a":{}}}),
            json!({"version":1,"workspaces":{"a":{"sessions":[session("ok"),invalid_time]}}}),
            json!({"version":1,"workspaces":{"a":{"sessions":[session("")]}}}),
        ] {
            assert!(
                decode(
                    &request(),
                    Timestamp::now(),
                    &serde_json::to_vec(&response).unwrap()
                )
                .is_err(),
                "{response}"
            );
        }
        for bytes in [
            r#"{"version":1,"workspaces":{"a":{"sessions":[]},"a":{"sessions":[]}}}"#,
            r#"{"version":1,"version":1,"workspaces":{}}"#,
            r#"{"version":1,"workspaces":{}} log"#,
            r#"{"version":1,"workspaces":{},"extension":{"a":1,"a":2}}"#,
        ] {
            assert!(decode(&request(), Timestamp::now(), bytes.as_bytes()).is_err());
        }
    }
}
