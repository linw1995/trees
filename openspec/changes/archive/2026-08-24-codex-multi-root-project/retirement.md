# Codex Launcher Retirement

## Decision

On 2026-09-23, Trees removed `trees codex`, `trees codex resume`, their
app-server Project synchronization code, and the Project checker. The archived
proposal, design, tasks, and capability spec preserve the original plan; they
do not describe current product behavior. The independent, read-only Codex
session provider and generic `trees open` program launcher remain available.
For example, `trees create --open=codex` still starts Codex as an ordinary
program in the workspace; it does not synchronize a multi-root Project.

## Missing Codex Capabilities

1. **One supported Project identity across app-server and Desktop.** The
   experimental app-server `project/create`, `project/import`, and
   `thread/start.projectId` operate on an app-server Project registry. Codex
   Desktop keeps a separate saved-Project catalog and sidebar assignment. A
   thread with a valid app-server Project ID can remain outside Desktop Projects,
   even when a client shares Desktop's app-server process. Trees cannot create
   or select a Desktop Project from its app-server Project ID.
2. **Project-aware Desktop thread creation and assignment.** Desktop does not
   expose an external operation that atomically creates a thread in a saved
   Project with an ordered runtime-root set, durable retry identity, and a
   verifiable result. Nor is there a supported external operation to assign an
   existing app-server thread to a Desktop Project. A successful
   `thread/start.projectId` response therefore cannot prove Desktop placement.
3. **Desktop thread activation.** The supported `codex app` command opens one
   path. The app-server protocol has no operation to open or focus an exact
   Desktop Project or thread in a window. `thread/resume` resumes a runtime
   session; it is not Desktop navigation. Trees could hand a thread to the CLI
   but could not provide the intended Desktop experience.
4. **Native customization discovery across independent roots.** Codex accepts
   `runtimeWorkspaceRoots`, but native `AGENTS.md` and project-hook discovery
   follows the primary working directory. The launcher supplied a generated
   manifest, which could not reproduce each repository's native instruction
   and hook behavior.
5. **Automatic restoration on ordinary resume.** A direct Codex resume does
   not recover Trees' managed root list or generated workspace manifest.
   Correct setup required a Trees-specific wrapper on every resume.

The CLI handoff implemented the narrower mechanics, but these gaps prevented
the desired multi-repository Desktop Project integration. Maintaining a
Codex-specific command around experimental APIs without that outcome was not
justified. A future design should begin with a supported Desktop Project
catalog, project-aware thread creation or assignment, exact thread activation,
and multi-root customization semantics; it should then define how resume
restores the same context.

This removal does not delete Projects or threads already created in a user's
Codex home. Trees has no authoritative inventory of those external records,
and removing the CLI requires no Trees database migration.

## Upstream Evidence

- [PR #38940](https://github.com/openai/codex/pull/38940) added the experimental app-server Project APIs and thread assignment.
- [Issue #40935](https://github.com/openai/codex/issues/40935) reproduces the separate Desktop and app-server Project assignments with a shared app-server process.
- [Issue #40535](https://github.com/openai/codex/issues/40535) requests an authorized Desktop Project catalog and exact thread activation bridge.
- [Issue #36250](https://github.com/openai/codex/issues/36250) requests atomic Desktop Project thread creation with runtime roots and idempotency.
- [Issue #38372](https://github.com/openai/codex/issues/38372) reproduces missing secondary-root instruction and hook discovery.
