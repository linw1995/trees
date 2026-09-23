#!/usr/bin/env python3

import re
import sys


VERSION = re.compile(
    r"^v(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


def version_key(tag):
    match = VERSION.fullmatch(tag)
    if match is None:
        return None

    major, minor, patch, prerelease = match.groups()
    if prerelease is None:
        suffix = (1,)
    else:
        identifiers = prerelease.split(".")
        if any(part.isdigit() and len(part) > 1 and part[0] == "0" for part in identifiers):
            return None
        suffix = (
            0,
            tuple((0, int(part)) if part.isdigit() else (1, part) for part in identifiers),
        )

    return (int(major), int(minor), int(patch), suffix)


def select_previous_release(target_tag, published_tags):
    target = version_key(target_tag)
    if target is None:
        raise ValueError(f"Invalid release tag: {target_tag}")

    candidates = []
    for tag in published_tags:
        version = version_key(tag)
        if version is None or version >= target:
            continue
        if target[3][0] == 1 and version[3][0] == 0:
            continue
        candidates.append((version, tag))

    return max(candidates, default=(None, ""))[1]


def main():
    if len(sys.argv) != 2:
        print("Usage: select_previous_release.py RELEASE_TAG", file=sys.stderr)
        return 2
    try:
        previous = select_previous_release(
            sys.argv[1], (line.strip() for line in sys.stdin)
        )
    except ValueError as error:
        print(error, file=sys.stderr)
        return 2
    print(previous)
    return 0


if __name__ == "__main__":
    sys.exit(main())
