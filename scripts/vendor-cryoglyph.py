#!/usr/bin/env python3
"""Reproduce the Iced 0.14-compatible Cryoglyph dependency backport."""

import argparse
import hashlib
import io
from pathlib import Path, PurePosixPath
import sys
import tarfile
import urllib.request


VERSION = "0.1.0"
ARCHIVE_URL = f"https://static.crates.io/crates/cryoglyph/cryoglyph-{VERSION}.crate"
ARCHIVE_SHA256 = "08bc795bdbccdbd461736fb163930a009da6597b226d6f6fce33e7a8eb6ec519"
ROOT = Path(__file__).resolve().parent.parent
DESTINATION = ROOT / "vendor" / "cryoglyph"


def expected_files(archive: bytes) -> dict[str, bytes]:
    digest = hashlib.sha256(archive).hexdigest()
    if digest != ARCHIVE_SHA256:
        raise ValueError(f"Cryoglyph archive checksum mismatch: {digest}")

    files = {}
    prefix = f"cryoglyph-{VERSION}"
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
        for member in package.getmembers():
            path = PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts or path.parts[0] != prefix:
                raise ValueError(f"Unexpected archive path: {member.name}")
            relative = path.relative_to(prefix)
            if member.isdir():
                continue
            if not member.isfile():
                raise ValueError(f"Unexpected archive member type: {member.name}")
            if relative.parts[0] not in {"src", "benches", "examples", "samples"} and str(relative) not in {
                "Cargo.toml",
                "Cargo.toml.orig",
                "README.md",
                "LICENSE-APACHE",
                "LICENSE-MIT",
                "LICENSE-ZLIB",
                ".cargo_vcs_info.json",
            }:
                continue
            key = str(relative)
            if key in files:
                raise ValueError(f"Duplicate archive path: {member.name}")
            source = package.extractfile(member)
            if source is None:
                raise ValueError(f"Missing archive content: {member.name}")
            files[key] = source.read()

    replacements = {
        "Cargo.toml": (
            b'[dependencies.lru]\nversion = "0.16"',
            b'[dependencies.lru]\nversion = "0.18.2"',
        ),
        "Cargo.toml.orig": (
            b'lru = { version = "0.16", default-features = false }',
            b'lru = { version = "0.18.2", default-features = false }',
        ),
    }
    for name, (before, after) in replacements.items():
        if files[name].count(before) != 1:
            raise ValueError(f"Unexpected upstream lru declaration in {name}")
        files[name] = files[name].replace(before, after)
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="Use an existing checksummed .crate archive")
    parser.add_argument("--check", action="store_true", help="Verify the backport without writing files")
    args = parser.parse_args()
    if args.archive:
        archive = args.archive.read_bytes()
    else:
        request = urllib.request.Request(ARCHIVE_URL, headers={"User-Agent": "modde-cryoglyph-backport"})
        with urllib.request.urlopen(request, timeout=30) as response:
            archive = response.read()
    files = expected_files(archive)
    extra = {
        str(path.relative_to(DESTINATION))
        for path in DESTINATION.rglob("*")
        if path.is_file()
    } - files.keys() - {"README.modde.md"}
    if extra:
        raise ValueError(f"Unexpected vendored files: {sorted(extra)}")
    for name, data in sorted(files.items()):
        destination = DESTINATION / name
        if destination.is_symlink():
            raise ValueError(f"Vendored path must not be a symlink: {name}")
        if args.check:
            if not destination.is_file() or destination.read_bytes() != data:
                raise ValueError(f"Vendored source drift: {name}")
        else:
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
    print(f"Cryoglyph {VERSION}: {len(files)} verified files; only the two lru declarations are patched")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, tarfile.TarError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
