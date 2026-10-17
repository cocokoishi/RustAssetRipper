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
# Every `*_tests_NN.rs` file is spliced in with `include!` and carries the parent module's
# indentation, so rustfmt must see it inside a module to judge it. This started as
# `^src/main_tests_[0-9]+\.rs$`, which covered only the first fragments written; there are now a
# dozen more under `src/**`, and for those the check would report every line as misformatted, so
# **any** edit to them failed CI regardless of what the edit did. Verified after widening:
# `serialized_file/metadata_tests_01.rs`, `io/stream_tests_01.rs`,
# `export/scene_wiring_tests_01.rs` and `processing/managed_assembly_tests_01.rs` are byte-identical
# to their wrapped rustfmt output. The remaining fragments still differ in a few lines, which the
# changed-range comparison tolerates as long as an edit does not touch one of them.
INCLUDED_TEST_FRAGMENT = re.compile(r"^src/(?:[^/]+/)*[A-Za-z0-9_]+_tests_[0-9]+\.rs$")


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


def format_source(path: str, source: list[str]) -> list[str] | None:
    # These files are included at module scope and carry the parent's four-space
    # indentation. Format them inside a temporary module so rustfmt sees that scope.
    wrapped = INCLUDED_TEST_FRAGMENT.fullmatch(path) is not None
    source_text = "".join(source)
    prefix = "mod __format_context {\n" if wrapped else ""
    suffix = "}\n" if wrapped else ""
    formatted = subprocess.run(
        ["rustfmt", "--edition", "2021", "--emit", "stdout"],
        input=(prefix + source_text + suffix).encode("utf-8"),
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if formatted.returncode != 0:
        return None
    lines = formatted.stdout.decode("utf-8").splitlines(keepends=True)
    if not wrapped:
        return lines
    if (
        len(lines) < 2
        or lines[0].strip() != "mod __format_context {"
        or lines[-1].strip() != "}"
    ):
        return None
    return lines[1:-1]


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
        formatted_lines = format_source(path, source)
        if formatted_lines is None:
            failures.append(path)
            continue
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
