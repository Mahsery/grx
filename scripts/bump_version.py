#!/usr/bin/env python3
"""
scripts/bump_version.py — Semantic Versioning Automation for grx

Embeds SemVer build metadata (+<git_short_hash>) into Cargo.toml.

Usage:
    python3 scripts/bump_version.py             # Update/append current git commit hash (e.g. 0.1.0+9f8912f)
    python3 scripts/bump_version.py --patch     # Bump patch and update hash (e.g. 0.1.1+9f8912f)
    python3 scripts/bump_version.py --minor     # Bump minor and update hash (e.g. 0.2.0+9f8912f)
    python3 scripts/bump_version.py --major     # Bump major and update hash (e.g. 1.0.0+9f8912f)
    python3 scripts/bump_version.py --no-hash   # Strip build metadata hash
    python3 scripts/bump_version.py --set 1.2.3 # Set exact version
    python3 scripts/bump_version.py --commit    # Bump and create git commit
    python3 scripts/bump_version.py --tag       # Bump, commit, and create git tag
    python3 scripts/bump_version.py --current   # Print current version
"""

import argparse
import hashlib
import os
import re
import subprocess
import sys
import time
from pathlib import Path


def find_cargo_toml() -> Path:
    current = Path(__file__).resolve().parent.parent / "Cargo.toml"
    if current.is_file():
        return current
    cwd = Path.cwd() / "Cargo.toml"
    if cwd.is_file():
        return cwd
    raise FileNotFoundError("Could not find Cargo.toml in repository root")


def get_git_hash() -> str:
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            stderr=subprocess.DEVNULL,
        )
        return out.decode().strip()
    except Exception:
        return hashlib.sha1(str(time.time()).encode()).hexdigest()[:7]


def get_current_version(cargo_path: Path) -> str:
    content = cargo_path.read_text(encoding="utf-8")
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', content)
    if not match:
        raise ValueError("Could not find version string in Cargo.toml")
    return match.group(1)


def parse_semver(version_str: str) -> tuple[int, int, int, str | None]:
    match = re.match(r"^(\d+)\.(\d+)\.(\d+)(?:\+([0-9a-zA-Z.-]+))?", version_str)
    if not match:
        raise ValueError(f"Version '{version_str}' does not follow semver (X.Y.Z[+metadata])")
    major = int(match.group(1))
    minor = int(match.group(2))
    patch = int(match.group(3))
    meta = match.group(4)
    return major, minor, patch, meta


def format_version(major: int, minor: int, patch: int, build_hash: str | None) -> str:
    base = f"{major}.{minor}.{patch}"
    if build_hash:
        return f"{base}+{build_hash}"
    return base


def update_cargo_toml(cargo_path: Path, old_ver: str, new_ver: str) -> None:
    content = cargo_path.read_text(encoding="utf-8")
    # Replace only the package version string in [package]
    pattern = re.compile(rf'(?m)^version\s*=\s*"{re.escape(old_ver)}"')
    new_content, count = pattern.subn(f'version = "{new_ver}"', content, count=1)
    if count == 0:
        raise ValueError(f"Failed to replace version {old_ver} in {cargo_path}")
    cargo_path.write_text(new_content, encoding="utf-8")


def sync_cargo_lock() -> None:
    try:
        subprocess.run(["cargo", "check", "--quiet"], check=False)
    except FileNotFoundError:
        pass


def git_commit_and_tag(new_ver: str, create_tag: bool) -> None:
    repo_root = Path(__file__).resolve().parent.parent
    files_to_add = ["Cargo.toml"]
    if (repo_root / "Cargo.lock").is_file():
        files_to_add.append("Cargo.lock")

    subprocess.run(["git", "add"] + files_to_add, cwd=repo_root, check=True)
    commit_msg = (
        f"release: v{new_ver}\n\n"
        f"Bump version to {new_ver} in Cargo.toml."
    )
    subprocess.run(["git", "commit", "-m", commit_msg], cwd=repo_root, check=True)
    print(f"git: committed release v{new_ver}")

    if create_tag:
        subprocess.run(["git", "tag", f"v{new_ver}"], cwd=repo_root, check=True)
        print(f"git: created tag v{new_ver}")


def main() -> int:
    parser = argparse.ArgumentParser(description="SemVer bump script with commit hash metadata for grx")
    parser.add_argument("--major", action="store_true", help="Bump major version (X.0.0+hash)")
    parser.add_argument("--minor", action="store_true", help="Bump minor version (x.Y.0+hash)")
    parser.add_argument("--patch", action="store_true", help="Bump patch version (x.y.Z+hash)")
    parser.add_argument("--no-hash", action="store_true", help="Do not append git commit hash metadata")
    parser.add_argument("--set", dest="exact", help="Set an exact version (e.g. 1.2.3+abc1234)")
    parser.add_argument("--current", action="store_true", help="Print current version and exit")
    parser.add_argument("--commit", action="store_true", help="Create git commit after bumping")
    parser.add_argument("--tag", action="store_true", help="Create git tag (implies --commit)")
    parser.add_argument("--dry-run", action="store_true", help="Display new version without writing")

    args = parser.parse_args()

    try:
        cargo_path = find_cargo_toml()
        current_ver = get_current_version(cargo_path)

        if args.current:
            print(current_ver)
            return 0

        major, minor, patch, _ = parse_semver(current_ver)
        build_hash = None if args.no_hash else get_git_hash()

        if args.exact:
            parse_semver(args.exact)
            new_ver = args.exact
        elif args.major:
            new_ver = format_version(major + 1, 0, 0, build_hash)
        elif args.minor:
            new_ver = format_version(major, minor + 1, 0, build_hash)
        elif args.patch:
            new_ver = format_version(major, minor, patch + 1, build_hash)
        else:
            # Default: refresh hash metadata while preserving major.minor.patch
            new_ver = format_version(major, minor, patch, build_hash)

        if args.dry_run:
            print(f"dry-run: {current_ver} -> {new_ver}")
            return 0

        update_cargo_toml(cargo_path, current_ver, new_ver)
        sync_cargo_lock()
        print(f"grx: updated version from {current_ver} to {new_ver}")

        if args.commit or args.tag:
            git_commit_and_tag(new_ver, create_tag=args.tag)

        return 0
    except Exception as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
