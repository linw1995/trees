#!/usr/bin/env python3

"""Verify the Codex Project associated with a Trees workspace."""

from __future__ import annotations

import argparse
import json
import os
import sqlite3
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import quote


WORKSPACE_METADATA_KEY = "treesWorkspaceId"


class CheckError(RuntimeError):
    pass


@dataclass
class Workspace:
    workspace_id: str
    path: Path
    state: str
    roots: list[Path]


def default_trees_database() -> Path:
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "trees" / "db.sqlite"
    if sys.platform.startswith("win"):
        local_app_data = os.environ.get("LOCALAPPDATA")
        base = Path(local_app_data) if local_app_data else Path.home() / "AppData" / "Local"
        return base / "trees" / "db.sqlite"
    state_home = os.environ.get("XDG_STATE_HOME")
    base = Path(state_home) if state_home else Path.home() / ".local" / "state"
    return base / "trees" / "db.sqlite"


def normalize_path(path: Path) -> Path:
    try:
        return path.expanduser().resolve(strict=False)
    except OSError:
        return path.expanduser().absolute()


def load_workspace(database_path: Path, workspace_path: Path) -> Workspace:
    database_path = normalize_path(database_path)
    if not database_path.exists():
        raise CheckError(f"Trees database does not exist: {database_path}")

    uri = f"file:{quote(str(database_path))}?mode=ro"
    try:
        with sqlite3.connect(uri, uri=True) as database:
            row = database.execute(
                """
                SELECT id, canonical_path, state
                FROM workspaces
                WHERE canonical_path = ?
                """,
                (str(normalize_path(workspace_path)),),
            ).fetchone()
            if row is None:
                raise CheckError(f"Workspace is not managed by Trees: {workspace_path}")

            roots = database.execute(
                """
                SELECT worktree_path
                FROM repo_worktrees
                WHERE workspace_id = ?
                ORDER BY worktree_path
                """,
                (row[0],),
            ).fetchall()
    except sqlite3.Error as error:
        raise CheckError(f"Failed to read Trees database {database_path}: {error}") from error

    return Workspace(
        workspace_id=row[0],
        path=Path(row[1]),
        state=row[2],
        roots=[Path(root[0]) for root in roots],
    )


class JsonRpcClient:
    def __init__(self, executable: Path, codex_home: Path | None) -> None:
        environment = os.environ.copy()
        if codex_home is not None:
            environment["CODEX_HOME"] = str(normalize_path(codex_home))
        try:
            self.process = subprocess.Popen(
                [str(executable), "app-server", "--stdio"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
                env=environment,
            )
        except OSError as error:
            raise CheckError(f"Failed to start {executable}: {error}") from error
        self.next_id = 1

    def request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        request_id = self.next_id
        self.next_id += 1
        self._write({"id": request_id, "method": method, "params": params})

        if self.process.stdout is None:
            raise CheckError("app-server stdout is unavailable")
        for line in self.process.stdout:
            if not line.strip():
                continue
            try:
                message = json.loads(line)
            except json.JSONDecodeError as error:
                raise CheckError(f"app-server returned malformed JSON: {line!r}") from error
            if message.get("id") != request_id:
                continue
            if "error" in message:
                raise CheckError(f"app-server rejected {method}: {message['error']}")
            result = message.get("result")
            if not isinstance(result, dict):
                raise CheckError(f"app-server returned an invalid {method} result")
            return result

        raise CheckError(f"app-server closed stdout while waiting for {method}")

    def notify(self, method: str, params: dict[str, Any]) -> None:
        self._write({"method": method, "params": params})

    def close(self) -> None:
        if self.process.stdin is not None:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()

    def _write(self, message: dict[str, Any]) -> None:
        if self.process.stdin is None:
            raise CheckError("app-server stdin is unavailable")
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()


def list_projects(client: JsonRpcClient) -> list[dict[str, Any]]:
    projects: list[dict[str, Any]] = []
    cursor: str | None = None
    for _ in range(1000):
        params: dict[str, Any] = {"limit": 100}
        if cursor is not None:
            params["cursor"] = cursor
        result = client.request("project/list", params)
        data = result.get("data")
        if not isinstance(data, list):
            raise CheckError("project/list returned an invalid data field")
        projects.extend(project for project in data if isinstance(project, dict))
        next_cursor = result.get("nextCursor")
        if next_cursor is None:
            return projects
        if not isinstance(next_cursor, str) or next_cursor == cursor:
            raise CheckError("project/list returned an invalid nextCursor")
        cursor = next_cursor
    raise CheckError("project/list exceeded the pagination limit")


def list_threads(client: JsonRpcClient, project_id: str) -> list[dict[str, Any]]:
    threads: list[dict[str, Any]] = []
    cursor: str | None = None
    for _ in range(1000):
        params: dict[str, Any] = {"limit": 100, "projectId": project_id}
        if cursor is not None:
            params["cursor"] = cursor
        result = client.request("thread/list", params)
        data = result.get("data")
        if not isinstance(data, list):
            raise CheckError("thread/list returned an invalid data field")
        threads.extend(thread for thread in data if isinstance(thread, dict))
        next_cursor = result.get("nextCursor")
        if next_cursor is None:
            return threads
        if not isinstance(next_cursor, str) or next_cursor == cursor:
            raise CheckError("thread/list returned an invalid nextCursor")
        cursor = next_cursor
    raise CheckError("thread/list exceeded the pagination limit")


def project_roots(project: dict[str, Any]) -> list[Path]:
    roots = project.get("roots")
    if not isinstance(roots, list):
        raise CheckError("project/list returned an invalid roots field")
    paths: list[Path] = []
    for root in roots:
        if not isinstance(root, dict) or not isinstance(root.get("path"), str):
            raise CheckError("project/list returned an invalid project root")
        paths.append(Path(root["path"]))
    return paths


def print_roots(label: str, roots: list[Path]) -> None:
    print(f"{label} ({len(roots)}):")
    for index, root in enumerate(roots, start=1):
        print(f"  {index}. {root}")


def check(args: argparse.Namespace) -> None:
    workspace = load_workspace(args.trees_db, args.workspace_path)
    expected_roots = [normalize_path(root) for root in workspace.roots]

    print(f"Trees workspace: {workspace.path}")
    print(f"Workspace ID: {workspace.workspace_id}")
    print(f"Workspace state: {workspace.state}")
    print_roots("Expected worktree roots", expected_roots)
    if workspace.state != "ready":
        raise CheckError(f"Workspace state is {workspace.state}, expected ready")
    if not expected_roots:
        raise CheckError("Workspace has no managed worktree roots")

    client = JsonRpcClient(args.codex_bin, args.codex_home)
    try:
        client.request(
            "initialize",
            {
                "clientInfo": {
                    "name": "trees-project-check",
                    "title": "Trees Project Check",
                    "version": "0.1.0",
                },
                "capabilities": {"experimentalApi": True},
            },
        )
        client.notify("initialized", {})
        projects = list_projects(client)
        owned_projects = [
            project
            for project in projects
            if project.get("metadata", {}).get(WORKSPACE_METADATA_KEY) == workspace.workspace_id
        ]
        if len(owned_projects) != 1:
            raise CheckError(
                f"Expected exactly one owned Codex Project, found {len(owned_projects)}"
            )

        project = owned_projects[0]
        project_id = project.get("id")
        if not isinstance(project_id, str) or not project_id:
            raise CheckError("Owned Codex Project has no valid id")
        actual_roots = [normalize_path(root) for root in project_roots(project)]

        print(f"Codex Project: {project.get('name', '<unnamed>')}")
        print(f"Project ID: {project_id}")
        print_roots("Project roots", actual_roots)
        if actual_roots != expected_roots:
            print("Project roots match: no")
            raise CheckError("Codex Project roots do not match Trees worktree roots")
        print("Project roots match: yes")

        threads = list_threads(client, project_id)
        print(f"Threads assigned to Project: {len(threads)}")
        for thread in threads:
            print(f"  - {thread.get('id', '<unknown>')} cwd={thread.get('cwd', '<unknown>')}")

        if args.thread_id is not None:
            result = client.request("thread/read", {"threadId": args.thread_id})
            thread = result.get("thread")
            if not isinstance(thread, dict):
                raise CheckError("thread/read returned an invalid thread")
            thread_project_id = thread.get("projectId")
            print(f"Checked thread: {args.thread_id}")
            print(f"Thread project ID: {thread_project_id}")
            if thread_project_id != project_id:
                raise CheckError("Thread is not assigned to the workspace Codex Project")
            print("Thread Project assignment: yes")
    finally:
        client.close()

    print("Result: PASS")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify a Trees workspace's Codex Project roots and optional thread assignment."
    )
    parser.add_argument("workspace_path", type=Path)
    parser.add_argument("--thread-id", help="Check that this thread is assigned to the workspace Project")
    parser.add_argument("--codex-bin", type=Path, default=Path("codex"))
    parser.add_argument("--codex-home", type=Path)
    parser.add_argument("--trees-db", type=Path, default=default_trees_database())
    return parser.parse_args()


def main() -> int:
    try:
        check(parse_args())
    except CheckError as error:
        print(f"Result: FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
