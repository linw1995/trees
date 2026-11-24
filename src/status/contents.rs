use std::fs;
use std::path::Path;

use serde::Serialize;

use super::WorkspaceStatus;
use crate::domain::{Timestamp, WorkspaceState};
use crate::validation::{self, WorkspaceRootError};

const MAX_ENTRIES: usize = 20;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Clean,
    Dirty,
    Missing,
    Removed,
    Unknown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Entry {
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    InstructionsUnreadable,
    RootUnreadable,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Observation {
    pub observed_at: Timestamp,
    pub state: State,
    pub changed_entries: Option<usize>,
    pub entries: Vec<Entry>,
    pub truncated: bool,
    pub issues: Vec<Issue>,
}

impl Observation {
    fn without_entries(observed_at: Timestamp, state: State) -> Self {
        Self {
            observed_at,
            state,
            changed_entries: None,
            entries: Vec::new(),
            truncated: false,
            issues: Vec::new(),
        }
    }
}

pub fn observe(target: Option<&WorkspaceStatus>) -> Option<Observation> {
    let target = target?;
    if target.repo_worktrees.len() == 1 && target.repo_worktrees[0].worktree_path == target.path {
        return None;
    }
    let observed_at = Timestamp::now();
    if target.state == WorkspaceState::Removed {
        return Some(Observation::without_entries(observed_at, State::Removed));
    }
    if !target.path.as_path().exists() {
        return Some(Observation::without_entries(observed_at, State::Missing));
    }
    Some(observe_container(observed_at, target))
}

fn observe_container(observed_at: Timestamp, target: &WorkspaceStatus) -> Observation {
    let expected = target
        .repo_worktrees
        .iter()
        .map(|worktree| worktree.worktree_path.as_path().to_owned())
        .collect::<Vec<_>>();
    match validation::unexpected_workspace_entries(target.path.as_path(), &expected) {
        Ok(paths) => {
            let changed_entries = paths.len();
            let entries = paths
                .iter()
                .take(MAX_ENTRIES)
                .map(|path| Entry {
                    path: relative_path(target.path.as_path(), path),
                    kind: entry_kind(path),
                })
                .collect::<Vec<_>>();
            Observation {
                observed_at,
                state: if changed_entries == 0 {
                    State::Clean
                } else {
                    State::Dirty
                },
                changed_entries: Some(changed_entries),
                truncated: changed_entries > entries.len(),
                entries,
                issues: Vec::new(),
            }
        }
        Err(error) => {
            let (code, message) = match error {
                WorkspaceRootError::Instructions { source } => {
                    (IssueCode::InstructionsUnreadable, Some(source.to_string()))
                }
                error => (IssueCode::RootUnreadable, Some(error.to_string())),
            };
            Observation {
                observed_at,
                state: State::Unknown,
                changed_entries: None,
                entries: Vec::new(),
                truncated: false,
                issues: vec![Issue {
                    code,
                    message: message.map(|message| bounded_message(&message)),
                }],
            }
        }
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn entry_kind(path: &Path) -> EntryKind {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => EntryKind::Symlink,
        Ok(metadata) if metadata.is_dir() => EntryKind::Directory,
        Ok(metadata) if metadata.is_file() => EntryKind::File,
        Ok(_) => EntryKind::Other,
        Err(_) => EntryKind::Other,
    }
}

fn bounded_message(message: &str) -> String {
    let mut output = String::new();
    let mut length = 0;
    for character in message.chars() {
        if length >= 200 {
            output.push('…');
            break;
        }
        if character.is_control() {
            for escaped in character.escape_default() {
                output.push(escaped);
                length += 1;
            }
        } else {
            output.push(character);
            length += 1;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::domain::{
        CanonicalPath, OriginRepositoryId, RepoWorktreeId, RepoWorktreeState, WorkspaceId,
        WorkspaceManagementMode,
    };
    use crate::status::disk_usage::CachedObservation;
    use crate::status::RepoWorktreeStatus;

    fn path(value: &Path) -> CanonicalPath {
        CanonicalPath::from_absolute(value).expect("test path should be absolute")
    }

    fn workspace(
        workspace_path: &Path,
        worktree_paths: &[&Path],
        state: WorkspaceState,
    ) -> WorkspaceStatus {
        WorkspaceStatus {
            workspace_id: WorkspaceId::new(),
            path: path(workspace_path),
            disk_usage: CachedObservation::default(),
            management_mode: WorkspaceManagementMode::Manual,
            state,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
            last_reconciled_at: None,
            last_released_at: None,
            removed_at: None,
            pool_id: None,
            claim: None,
            current_operation: None,
            repo_worktrees: worktree_paths
                .iter()
                .map(|worktree_path| RepoWorktreeStatus {
                    repo_worktree_id: RepoWorktreeId::new(),
                    origin_repository_id: OriginRepositoryId::new(),
                    source_path: path(worktree_path),
                    worktree_path: path(worktree_path),
                    disk_usage: CachedObservation::default(),
                    state: RepoWorktreeState::Attached,
                    last_head: None,
                    last_observed_at: Timestamp::now(),
                })
                .collect(),
        }
    }

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-status-contents-{}", WorkspaceId::new()))
    }

    #[test]
    fn reports_unexpected_container_entries() {
        let root = test_root();
        let worktree = root.join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(root.join("notes.md"), "notes\n").unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        crate::workspace_instructions::create(&root).unwrap();
        let workspace = workspace(&root, &[&worktree], WorkspaceState::Ready);

        let observation = observe(Some(&workspace)).expect("observation should run");
        assert_eq!(observation.state, State::Dirty);
        assert_eq!(observation.changed_entries, Some(2));
        assert_eq!(
            observation
                .entries
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            ["notes.md", "scripts"]
        );
        assert_eq!(observation.entries[0].kind, EntryKind::File);
        assert_eq!(observation.entries[1].kind, EntryKind::Directory);
        assert!(!observation.truncated);

        fs::remove_file(root.join("notes.md")).unwrap();
        fs::remove_dir(root.join("scripts")).unwrap();
        let observation = observe(Some(&workspace)).expect("observation should run");
        assert_eq!(observation.state, State::Clean);
        assert_eq!(observation.changed_entries, Some(0));
        assert!(observation.entries.is_empty());

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn bounds_reported_entries() {
        let root = test_root();
        let worktree = root.join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        for index in 0..MAX_ENTRIES + 1 {
            fs::write(root.join(format!("file-{index:02}")), "content\n").unwrap();
        }
        let workspace = workspace(&root, &[&worktree], WorkspaceState::Ready);
        let observation = observe(Some(&workspace)).expect("observation should run");
        assert_eq!(observation.changed_entries, Some(MAX_ENTRIES + 1));
        assert_eq!(observation.entries.len(), MAX_ENTRIES);
        assert!(observation.truncated);
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn skips_a_single_worktree_root() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let workspace = workspace(&root, &[&root], WorkspaceState::Ready);
        assert_eq!(observe(Some(&workspace)), None);
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn reports_missing_removed_and_unknown_roots() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let missing = workspace(
            &root.join("missing"),
            &[&root.join("worktree")],
            WorkspaceState::Ready,
        );
        let observation = observe(Some(&missing)).expect("observation should run");
        assert_eq!(observation.state, State::Missing);
        assert_eq!(observation.changed_entries, None);

        let mut removed = missing;
        removed.state = WorkspaceState::Removed;
        let observation = observe(Some(&removed)).expect("observation should run");
        assert_eq!(observation.state, State::Removed);

        let not_a_directory = root.join("file");
        fs::write(&not_a_directory, "content\n").unwrap();
        let invalid = workspace(
            &not_a_directory,
            &[&root.join("worktree")],
            WorkspaceState::Ready,
        );
        let observation = observe(Some(&invalid)).expect("observation should run");
        assert_eq!(observation.state, State::Unknown);
        assert_eq!(observation.issues[0].code, IssueCode::RootUnreadable);

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn absent_target_observation_returns_none() {
        assert_eq!(observe(None), None);
    }

    #[test]
    fn classifies_entry_kinds_and_bounds_issue_messages() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let file = root.join("file.txt");
        fs::write(&file, "content\n").unwrap();
        assert_eq!(entry_kind(&file), EntryKind::File);
        assert_eq!(entry_kind(&root), EntryKind::Directory);
        assert_eq!(entry_kind(&root.join("absent")), EntryKind::Other);
        #[cfg(unix)]
        {
            let link = root.join("link");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert_eq!(entry_kind(&link), EntryKind::Symlink);
        }
        assert_eq!(bounded_message("line\n\u{1b}[31m"), "line\\n\\u{1b}[31m");
        let long = "a".repeat(250);
        let bounded = bounded_message(&long);
        assert!(bounded.ends_with('…'));
        assert_eq!(bounded.chars().count(), 201);
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[cfg(unix)]
    #[test]
    fn reports_unreadable_instructions() {
        use std::os::unix::fs::PermissionsExt;

        let root = test_root();
        let worktree = root.join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        crate::workspace_instructions::create(&root).unwrap();
        let instructions = root.join(crate::workspace_instructions::FILE_NAME);
        fs::set_permissions(&instructions, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&instructions).is_err() {
            let workspace = workspace(&root, &[&worktree], WorkspaceState::Ready);
            let observation = observe(Some(&workspace)).expect("observation should run");
            assert_eq!(observation.state, State::Unknown);
            assert_eq!(
                observation.issues[0].code,
                IssueCode::InstructionsUnreadable
            );
            assert!(observation.issues[0].message.is_some());
        }
        fs::set_permissions(&instructions, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_dir_all(root).expect("test root should be removable");
    }
}
