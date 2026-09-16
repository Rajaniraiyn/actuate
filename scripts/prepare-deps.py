#!/usr/bin/env python3
"""Materialize pinned upstream source plus reviewed patches. Python 3.9+, git."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "target" / "patched-deps"
LIMIT = 32 * 1024 * 1024
STAMP = ".actuate-source.json"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def inventory(directory):
    result = {}
    for path in sorted(directory.rglob("*")):
        relative = path.relative_to(directory).as_posix()
        if relative == STAMP:
            continue
        if path.is_symlink():
            raise RuntimeError(f"Unexpected symlink in generated source: {path}")
        if path.is_file():
            result[relative] = digest(path.read_bytes())
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="Use a previously downloaded pinned archive offline")
    args = parser.parse_args()
    manifest_bytes = (ROOT / "patches/manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    patches = [(ROOT / "patches" / f"{crate}.patch").read_bytes() for crate in manifest["crates"]]
    identity = digest(manifest_bytes + b"".join(patches))
    if DEST.exists():
        stamp_path = DEST / STAMP
        if not stamp_path.is_file():
            raise RuntimeError(f"Refusing to replace unrecognized directory: {DEST}")
        stamp = json.loads(stamp_path.read_text())
        if inventory(DEST) != stamp["files"]:
            raise RuntimeError(f"Generated source has local changes; preserve or remove {DEST} manually")
        if stamp["identity"] == identity:
            print(f"Patched dependencies are current: {DEST}")
            return
    if args.archive:
        with args.archive.open("rb") as source:
            archive = source.read(LIMIT + 1)
    else:
        with urllib.request.urlopen(manifest["archive"], timeout=30) as source:
            archive = source.read(LIMIT + 1)
    if len(archive) > LIMIT or digest(archive) != manifest["sha256"]:
        raise RuntimeError("Upstream archive exceeds limit or SHA-256 does not match the pin")
    DEST.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".patched-deps-", dir=DEST.parent) as temp:
        staging = Path(temp) / "source"
        staging.mkdir()
        prefix = f"DroidMux-{manifest['revision']}"
        expanded = 0
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
            for member in tar:
                parts = PurePosixPath(member.name).parts
                if not parts or parts[0] != prefix or ".." in parts:
                    raise RuntimeError(f"Invalid archive path: {member.name}")
                destinations = []
                if len(parts) >= 4 and parts[1] == "crates" and parts[2] in manifest["crates"]:
                    destinations = [staging.joinpath(*parts[2:])]
                elif len(parts) == 2 and parts[1] in ("LICENSE-MIT", "LICENSE-APACHE"):
                    destinations = [staging / crate / parts[1] for crate in manifest["crates"]]
                if not destinations or member.isdir():
                    continue
                if not member.isfile():
                    raise RuntimeError(f"Links and special files are unsupported: {member.name}")
                expanded += member.size * len(destinations)
                if expanded > LIMIT:
                    raise RuntimeError("Extracted source exceeds size limit")
                data = tar.extractfile(member).read()
                for destination in destinations:
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    if destination.exists():
                        if destination.read_bytes() != data:
                            raise RuntimeError(f"Conflicting archive entry: {destination}")
                    else:
                        with destination.open("xb") as output:
                            output.write(data)
        for crate in manifest["crates"]:
            patch = ROOT / "patches" / f"{crate}.patch"
            subprocess.run(["git", "apply", "--check", str(patch)], cwd=staging, check=True)
            subprocess.run(["git", "apply", str(patch)], cwd=staging, check=True)
        (staging / STAMP).write_text(json.dumps({"identity": identity, "files": inventory(staging)}, indent=2) + "\n")
        # Recheck before replacing a previously generated tree. Never discard edits.
        if DEST.exists():
            if inventory(DEST) != json.loads((DEST / STAMP).read_text())["files"]:
                raise RuntimeError("Generated source changed while preparing dependencies")
            shutil.rmtree(DEST)
        os.replace(staging, DEST)
    print(f"Prepared patched dependencies: {DEST}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f"prepare-deps: {exc}") from exc
