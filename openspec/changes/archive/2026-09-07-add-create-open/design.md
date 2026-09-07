## Context

A child process cannot change its parent shell directory. It can, however, start a program with the workspace as its current directory. Where process replacement is supported, replacing Trees with that program avoids leaving an intermediate Trees process alive for the duration of an interactive session.

## Goals / Non-Goals

**Goals:**

- Open an interactive shell in a newly created or allocated workspace with one option.
- Allow callers to select another executable without invoking a command shell.
- Preserve terminal, signal, and environment inheritance during the handoff.
- Validate the default program before workspace mutation.

**Non-Goals:**

- Change the parent shell directory.
- Parse a shell command string or support program arguments in the first version.
- Release an automatic workspace when the opened program exits.
- Change create output when `--open` is absent.

## Decisions

### Model Open as an Optional Program

The create parser represents `--open` as an option with an optional value. A missing value resolves from `$SHELL`; an explicit value identifies one executable. Explicit values use `--open=<PROGRAM>` so the optional program cannot consume the optional positional workspace path.

Trees rejects an unset or empty `$SHELL` and an explicitly empty program before creating or allocating a workspace. It also rejects `--open` with `--json`, because process handoff and machine-readable result output are incompatible invocation modes.

### Execute the Program in the Workspace

Trees sets the child current directory through the process builder rather than changing its own global current directory. Where process replacement is supported, `exec` replaces Trees while preserving inherited standard streams, environment, process group, and signal behavior. Other platforms spawn the program and propagate its exit status as a compatibility fallback.

The program receives no implicit arguments. Shells use their normal interactive detection, while tools such as Codex can infer the workspace from their current directory.

### Keep the Workspace Claim After Exit

Automatic create retains its existing active claim. When `exec` removes Trees from the process image, Trees does not observe program exit and cannot safely perform an automatic release. The user releases the workspace explicitly, including through `trees release` from inside the workspace.

## Risks / Trade-Offs

- [Users may expect the original shell directory to change] -> Document that exiting the opened program returns to the original shell and directory.
- [The selected executable may be absent] -> Report the program and allocated workspace path if execution fails; keep the successfully created workspace intact.
- [An opened shell may not be interactive when standard input is not a terminal] -> Preserve native shell behavior instead of forcing shell-specific arguments.
- [Program arguments are not supported] -> Keep the first contract unambiguous and allow a later explicit argument boundary if needed.

## Migration Plan

No migration is required. The new option is additive and existing create invocations retain their output and lifecycle behavior.
