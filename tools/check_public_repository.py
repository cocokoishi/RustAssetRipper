#!/usr/bin/env python3
"""Reject private input files and oversized blobs from the public Git tree.

This checks Git object path and size metadata only. It never opens file bodies,
and failure output intentionally omits paths, names, hashes, and file contents.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import PurePosixPath


MAX_BLOB_BYTES = 25 * 1024 * 1024
PRIVATE_SUFFIXES = {
    ".assetbundle",
    ".bundle",
    ".unity3d",
    ".unitypackage",
    ".vrca",
    ".vrcw",
}
PRIVATE_CREDENTIAL_SUFFIXES = {".key", ".pem", ".p12", ".pfx", ".jks"}


def run_git(*args: str, input_bytes: bytes | None = None) -> bytes:
    return subprocess.run(
        ["git", *args],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        input=input_bytes,
    ).stdout


def is_private_path(path: PurePosixPath, blob_size: int) -> bool:
    lowered_parts = tuple(part.casefold() for part in path.parts)
    lowered_name = path.name.casefold()
    suffix = path.suffix.casefold()

    if suffix == ".bundle":
        is_public_fixture = lowered_parts[:3] == (
            "src",
            "export",
            "managed_reference_project_fixture",
        )
        return not is_public_fixture or blob_size > 1024 * 1024
    if suffix in PRIVATE_SUFFIXES or suffix in PRIVATE_CREDENTIAL_SUFFIXES:
        return True
    if lowered_name == ".env" or (
        lowered_name.startswith(".env.") and lowered_name != ".env.example"
    ):
        return True
    if "artifacts" in lowered_parts:
        return True
    return (
        len(lowered_parts) >= 4
        and lowered_parts[0] == "tools"
        and lowered_parts[1] == "probes"
        and lowered_parts[3] == "packages"
    )


def main() -> int:
    try:
        history_entries = run_git(
            "-c", "core.quotePath=false", "rev-list", "--objects", "--all"
        ).splitlines()
        history: list[tuple[PurePosixPath | None, str]] = []
        for entry in history_entries:
            object_id, separator, raw_path = entry.partition(b" ")
            path = (
                PurePosixPath(raw_path.decode("utf-8", errors="surrogateescape"))
                if separator
                else None
            )
            history.append((path, object_id.decode("ascii")))

        metadata_raw = run_git(
            "cat-file",
            "--batch-check=%(objecttype) %(objectsize)",
            input_bytes=("\n".join(object_id for _, object_id in history) + "\n").encode(),
        ) if history else b""
        metadata = [line.split() for line in metadata_raw.splitlines()]
        if len(metadata) != len(history):
            raise RuntimeError

        private_paths = sum(
            path is not None and is_private_path(path, int(metadata[index][1]))
            for index, (path, _) in enumerate(history)
        )
        oversized_blobs = sum(
            kind == b"blob" and int(size) > MAX_BLOB_BYTES
            for kind, size in metadata
        )

        if private_paths or oversized_blobs:
            print(
                "Public repository policy failed: "
                f"{private_paths} private-path entries, "
                f"{oversized_blobs} oversized Git blobs. "
                "The check intentionally does not print file names or contents.",
                file=sys.stderr,
            )
            return 1

        print(
            "Public repository policy passed: tracked Git metadata contains no "
            "private input paths or oversized blobs."
        )
        return 0
    except (OSError, subprocess.CalledProcessError, ValueError, RuntimeError):
        print(
            "Public repository policy check could not inspect the Git index.",
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
