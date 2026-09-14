## Context

| Consumer | Current selection | Behavior to preserve |
| --- | --- | --- |
| Status | ID or nearest ancestor of current directory | Removed boundaries participate; unmatched current directory is allowed; absent storage stays absent |
| Release | Exact path, nearest ancestor of current directory, or claim ID | Snapshot the selected claim; fail-fast admission must reject a replacement claim |
| Open | Workspace ID | Check workspace, claim, and lease within one read-only transaction |
| Managed-session preparation | Exact path from forwarded arguments, defaulting to `.` | Preserve exact-root semantics and existing reconciliation |
| Remove | Workspace-or-repository ID | Detect cross-table ambiguity within its existing transaction |

`status::target::TargetSelector` and release's identity helpers duplicate
containing-workspace lookup using different algorithms. Direct ID/path queries
elsewhere are not all duplicates: creation collision checks and transactional
validation remain storage operations.

## Decisions

### Represent One Selection Strategy as an Enum

Add `src/workspace_locator.rs` and export it from `src/lib.rs`:

```rust
pub enum WorkspaceSelector {
    Id(WorkspaceId),
    ExactPath(CanonicalPath),
    ContainingDirectory(CanonicalPath),
    ClaimId(ClaimId),
}

pub struct LocatedWorkspace {
    pub workspace: WorkspaceRow,
    pub selected_claim_id: Option<ClaimId>,
}

pub fn locate(
    connection: &mut SqliteConnection,
    selector: &WorkspaceSelector,
) -> Result<Option<LocatedWorkspace>, LocateError>;
```

The enum makes conflicting strategies impossible to represent after parsing. Do not use
an options struct with precedence rules. `selected_claim_id` is populated only
when lookup traverses a claim, preserving that exact identity for admission.
It is provenance, not a claim-validity or ownership guarantee.

Keep the concrete function rather than introducing a trait. SQLite queries can
be tested with the existing in-memory database fixtures.

### Normalize Inputs Before Lookup

Typed ID parsing and path normalization happen at the command boundary before
opening storage. Locator lookup does not read current directory or canonicalize persisted
paths. Use `validation::resolve_workspace_path` for explicit exact paths,
preserving its existing support for a missing final path component. Use
`CanonicalPath::resolve` for current directory, preserving canonical path resolution failures and
symlink handling. Do not silently expand support for arbitrary missing parents.

### Use One Containing-Directory Algorithm

Walk canonical directory ancestors from nearest to root and reuse
`find_workspace_by_path`. This avoids loading the full workspace inventory and
uses exact component boundaries. Include all persisted workspace states.
Return the first registered workspace even if removed, manual, unclaimed, or
otherwise ineligible for the caller's operation; never fall back to an outer
workspace after a business-rule failure.

Exact paths never fall back to ancestors. ID and claim lookup require no
workspace filesystem access. Claim lookup reads the active claim row and then
its workspace without replacing the requested claim with a newer one.

### Distinguish Absence from Failure

`Ok(None)` means no matching workspace or no matching claim. Query failures
remain typed Snafu errors with their original sources. A dangling claim is an
integrity failure, not an ordinary unmatched selector. Selector context remains
available to callers for their existing unknown-ID, path-not-found, and
claim-not-found errors.

Status alone permits an unmatched implicit current directory. Every explicit selector must
fail when unmatched; no fallback to current directory or another strategy is permitted.
Status's no-database branch stays in its adapter: implicit current directory produces no
target, and explicit input produces a not-found error, without creating storage.

Preserve existing CLI error text and exit behavior using narrow typed adapters.
Use `.context(...)`, transparent propagation, and source-less selectors as
appropriate. Retain explicit classification where existing error variants or
cleanup behavior require it; never stringify errors across module boundaries.

### Keep Transaction Ownership with the Caller

The locator starts no transaction and performs no lifecycle mutation. Callers
requiring a coherent multi-query view must invoke it in their existing snapshot.

- Status selects the target inside the transaction that loads inventory and
  process boundaries. Process observation still happens after storage closes.
- Open locates and loads `find_workspace_open_snapshot` in the same existing
  transaction, then retains every removed, lease, and automatic-claim check.
- Release obtains the selected workspace and claim identity before admission.
  Path/ID/current directory selection snapshots the workspace's active claim; claim selection
  preserves `selected_claim_id`. Existing admission validates again the exact claim
  and keeps the try-once busy behavior. No second lookup may switch targets or
  adopt a replacement claim.
- Managed-session preparation delegates only its initial exact-path lookup.
  Recovery, operation admission, reconciliation, and worktree checks remain local.

Remove retains its heterogeneous resolver. Do not replace its collision check
with a workspace-first lookup. Creation checks and post-admission rereads also
remain direct storage queries.

### CLI Integration

Introduce a CLI-side abstraction in `src/cli/workspace_locator.rs`, separate
from the storage-facing locator. Reuse both argument definitions and conversion
logic, rather than only flattening three fields into each command.

- `WorkspaceLocatorArgs` owns the three named options, their value names, help,
  and shared parsing behavior.
- A shared group constructor owns the mutually exclusive argument membership.
  It accepts the command's explicit-selection policy. Both shared named fields
  and command-local legacy positional fields declare membership in the shared
  group, avoiding copied option lists or pairwise conflict declarations. Wire this through the parser construction path
  used by both production and parser tests, including generated help.
- `WorkspaceLocatorInput` represents one explicit, raw input as
  `Id(WorkspaceId)`, `ExactPath(PathBuf)`, or `ClaimId(ClaimId)`. A command's
  positional compatibility adapter produces this same type.
- `SelectionDefault` is an enum with `RequireExplicit` and `CurrentDirectory`.
  It is shared by parser configuration and conversion, preventing their
  required/default behavior from drifting.
- One conversion entry point merges named input and optional legacy input,
  rejects multiple selections, applies the default, and normalizes paths into
  `WorkspaceSelector`. It uses typed Snafu errors and performs no database access.

Reject conflicting input before path normalization or current directory access, including
when argument structs are constructed directly without Clap. Even two inputs
naming the same target are conflicting; never use precedence. Reuse ID parsing
and preserve existing diagnostics with narrow adapters where necessary.

The raw optional argument fields exist only at the parser boundary. Downstream
command handlers receive one selector and must not duplicate `Option` matching,
claim parsing, current directory fallback, or canonical path resolution. Command-specific not-found and
eligibility behavior remains outside this CLI abstraction. Avoid a generic
command framework or trait hierarchy; a shared argument type, enums, and small
functions cover the actual variation.

| Command | Existing positional input | New named selectors | No selector |
| --- | --- | --- | --- |
| `status` | Optional workspace ID | `--workspace-id`, `--workspace-dir`, `--claim-id` | Nearest workspace containing current directory; no match is allowed |
| `open` | Workspace ID | `--workspace-id`, `--workspace-dir`, `--claim-id` | Parser error; one explicit selector is required |
| `release` | Optional exact workspace directory | `--workspace-id`, `--workspace-dir`, `--claim-id` | Nearest workspace containing current directory; no match is an error |

`--workspace-dir` always means an exact registered root. The containing-directory
strategy is used by existing implicit-current directory adapters; an additional public flag
is not necessary for this change. Legacy positional values keep their original
types; do not guess whether a string is a path, ID, or name. Preserve the existing
invalid-claim diagnostic when centralizing parsing.

Status headings identify ID, path, claim, or current-directory selection without
changing existing headings. Inventory filtering and JSON structure remain
unchanged. Session forwarding parsing and remove receive no new flags.

## Migration and Risks

Implement lookup and migrate existing consumers before adding CLI syntax, so
semantic regressions can be distinguished from intentional interface additions.
Remove the old target enum and ancestor algorithms once all callers migrate;
temporary compatibility wrappers may delegate but must not retain algorithms.

The primary risks are accidentally skipping removed boundaries, adopting a new
claim during release, splitting status/open snapshots, changing exact-path
semantics, and consuming forwarded session flags. Regression tests target these
contracts. No persisted-data migration is needed; rollback is a code revert.
