#!/usr/bin/env python3
"""Check rustfmt output only where this change edits Rust source."""

from __future__ import annotations

import difflib
import os
import re
import subprocess
import sys
from pathlib import Path


HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")
ZERO_SHA = "0" * 40


def git(*args: str) -> bytes:
    return subprocess.run(
        ["git", *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    ).stdout


def select_base() -> str:
    event = os.environ.get("GITHUB_EVENT_NAME", "")
    candidate = (
        os.environ.get("PR_BASE_SHA", "")
        if event == "pull_request"
        else os.environ.get("PUSH_BEFORE_SHA", "")
    )
    if re.fullmatch(r"[0-9a-fA-F]{40}", candidate) and candidate != ZERO_SHA:
        return candidate
    parent = subprocess.run(
        ["git", "rev-parse", "HEAD^"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    ).stdout.strip()
    return parent or "HEAD"


def changed_ranges(patch: str) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    for line in patch.splitlines():
        match = HUNK.match(line)
        if not match:
            continue
        start = int(match.group(1))
        count = int(match.group(2) or "1")
        # Include adjacent lines because formatting can wrap around a changed token.
        ranges.append((max(1, start - 1), max(1, start + count)))
    return ranges


def intersects(start: int, end: int, ranges: list[tuple[int, int]]) -> bool:
    return any(start <= changed_end and changed_start <= end for changed_start, changed_end in ranges)


def format_changes_touch_changed_lines(
    source: list[str], formatted: list[str], ranges: list[tuple[int, int]]
) -> bool:
    matcher = difflib.SequenceMatcher(a=source, b=formatted, autojunk=False)
    for tag, old_start, old_end, _, _ in matcher.get_opcodes():
        if tag == "equal":
            continue
        if old_start == old_end:
            line = old_start + 1
            if intersects(line, line, ranges):
                return True
        elif intersects(old_start + 1, old_end, ranges):
            return True
    return False


def main() -> int:
    base = select_base()
    head = os.environ.get("GITHUB_SHA", "HEAD")
    comparison = f"{base}...{head}"
    paths = git(
        "diff", "--name-only", "-z", comparison, "--", "*.rs"
    ).decode("utf-8", errors="surrogateescape").split("\0")
    failures: list[str] = []

    for path in filter(None, paths):
        patch = git(
            "diff", "--no-ext-diff", "--unified=0", comparison, "--", path
        ).decode("utf-8", errors="surrogateescape")
        ranges = changed_ranges(patch)
        if not ranges:
            continue

        source_path = Path(path)
        source = source_path.read_text(encoding="utf-8").splitlines(keepends=True)
        formatted = subprocess.run(
            ["rustfmt", "--edition", "2021", "--emit", "stdout", path],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if formatted.returncode != 0:
            failures.append(path)
            continue
        formatted_lines = formatted.stdout.decode("utf-8").splitlines(keepends=True)
        if format_changes_touch_changed_lines(source, formatted_lines, ranges):
            failures.append(path)

    if failures:
        print(
            "rustfmt found formatting changes in edited Rust lines: "
            + ", ".join(failures),
            file=sys.stderr,
        )
        return 1
    print("rustfmt check passed for edited Rust lines.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
