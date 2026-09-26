#!/usr/bin/env python3
"""Validate a Conventional Commits header in a commit message or PR title."""

import argparse
import os
from pathlib import Path
import re
import sys


HEADER = re.compile(r"[a-z]+(?:\([a-zA-Z0-9][a-zA-Z0-9./_-]*\))?!?: \S[^\r\n]*")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("message_file", nargs="?", type=Path)
    parser.add_argument("--pr-title", action="store_true")
    args = parser.parse_args()

    if args.pr_title == (args.message_file is not None):
        parser.error("provide exactly one of a message file or --pr-title")

    if args.pr_title:
        title = os.environ.get("PR_TITLE", "")
        source = "PR title"
    else:
        title = args.message_file.read_text(encoding="utf-8").split("\n", 1)[0]
        source = "commit message"

    if HEADER.fullmatch(title) and title == title.rstrip():
        return 0

    print(
        f"Invalid {source}: {title!r}\n"
        "Expected Conventional Commits format: <type>[optional scope][!]: <description>",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
