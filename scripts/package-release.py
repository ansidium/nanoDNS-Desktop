#!/usr/bin/env python3
"""Create allowlisted portable and corresponding-source ZIPs using Python 3.11+.

The corresponding project source is retained in the separate source ZIP.
No signing, network access, Cargo invocation, or publication is performed.
Existing release artifacts are never overwritten.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import fnmatch
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import struct
import tempfile
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[1]
SOURCE_DIRECTORIES = (
    ".cargo", "src", "assets", "locales", "vendor", "third_party", "scripts", "docs",
    "packaging",
)
SOURCE_FILES = (
    "Cargo.lock", "Cargo.toml", "build.rs", "build.ps1", "verify-release.py",
    "DESIGN.md", "CONTRIBUTING.md", "CHANGELOG.md", "LICENSE",
    "THIRD_PARTY_NOTICES.txt", ".gitignore", ".gitattributes",
    ".github/workflows/ci.yml",
)
PORTABLE_FILES = {
    "README.txt": "packaging/README.txt",
    "LICENSE": "LICENSE",
    "THIRD_PARTY_NOTICES.txt": "THIRD_PARTY_NOTICES.txt",
    "licenses/eframe/LICENSE-MIT": "vendor/eframe/LICENSE-MIT",
    "licenses/eframe/LICENSE-APACHE": "vendor/eframe/LICENSE-APACHE",
    "licenses/eframe/NANODNS-PATCH.md": "vendor/eframe/NANODNS-PATCH.md",
}
EXCLUDED_DIRECTORIES = {
    ".git", ".impeccable", ".font-build", ".cache", ".pytest_cache", ".mypy_cache", ".ruff_cache",
    "__pycache__", "target", "dist", "qa", "graft", "node_modules",
}
EXCLUDED_FILES = {
    "settings.json", "credentials", "credentials.toml", "credential", ".ds_store",
    "thumbs.db", "desktop.ini",
}
EXCLUDED_PATTERNS = (
    ".env", ".env.*", "*.pyc", "*.pyo", "*.pdb", "*.exe", "*.dll", "*.lib",
    "*.obj", "*.log", "*.log.*", "*.sddl", "*.dpapi.xml", "*.pfx", "*.p12",
    "*.pem", "*.key", "*.crt", "*.cer", "*.token", "*.secret", "*.bak", "*.tmp",
    "*.zip", "*.7z", "*.rar", "*.tar", "*.gz", "*.xz",
    "*credentials*", "*private-key*", "*signing-key*",
)
ZIP_TIMESTAMP = (1980, 1, 1, 0, 0, 0)


@dataclass(frozen=True)
class Entry:
    name: str
    data: bytes

    @property
    def sha256(self) -> str:
        return hashlib.sha256(self.data).hexdigest()


def is_link(path: Path) -> bool:
    return path.is_symlink() or (hasattr(path, "is_junction") and path.is_junction())


def excluded(relative: PurePosixPath) -> bool:
    parts = tuple(part.casefold() for part in relative.parts)
    if any(part in EXCLUDED_DIRECTORIES or part.startswith(".venv") for part in parts[:-1]):
        return True
    name = parts[-1]
    return name in EXCLUDED_FILES or any(fnmatch.fnmatchcase(name, pattern) for pattern in EXCLUDED_PATTERNS)


def read_entry(root: Path, relative: str) -> Entry:
    path = root / relative
    if is_link(path) or not path.is_file():
        raise ValueError(f"Required regular source file is missing or linked: {relative}")
    if not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f"Source file leaves the project directory: {relative}")
    return Entry(relative, path.read_bytes())


def source_entries(root: Path) -> list[Entry]:
    names = set(SOURCE_FILES)
    for pattern in ("README*.md", "README*.txt"):
        names.update(path.name for path in root.glob(pattern) if path.is_file())
    names.update(PORTABLE_FILES.values())
    for directory in SOURCE_DIRECTORIES:
        base = root / directory
        if is_link(base) or not base.is_dir():
            raise ValueError(f"Required source directory is missing or linked: {directory}")
        for current, directories, files in os.walk(base, followlinks=False):
            current_path = Path(current)
            allowed_directories = []
            for name in sorted(directories):
                child = current_path / name
                relative = child.relative_to(root).as_posix()
                if name.casefold() in EXCLUDED_DIRECTORIES or name.casefold().startswith(".venv"):
                    continue
                if is_link(child):
                    raise ValueError(f"Linked source directory is not allowed: {relative}")
                allowed_directories.append(name)
            directories[:] = allowed_directories
            for name in sorted(files):
                relative = (current_path / name).relative_to(root).as_posix()
                if not excluded(PurePosixPath(relative)):
                    names.add(relative)
    return [read_entry(root, name) for name in sorted(names)]


def validate_unsigned_pe(data: bytes) -> dict[str, str | int]:
    """Reject malformed, non-x64, non-GUI and certificate-bearing PE images."""
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("The executable is not a DOS/PE image")
    pe = struct.unpack_from("<I", data, 60)[0]
    if pe < 64 or pe + 24 > len(data) or data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("Invalid PE header")
    coff = pe + 4
    machine, sections = struct.unpack_from("<HH", data, coff)
    optional_size, characteristics = struct.unpack_from("<HH", data, coff + 16)
    optional = coff + 20
    if machine != 0x8664 or not 1 <= sections <= 96:
        raise ValueError("Expected a Windows x64 executable")
    if not characteristics & 0x0002 or characteristics & 0x2000:
        raise ValueError("Expected an executable image, not a DLL")
    if optional_size < 152 or optional + optional_size + sections * 40 > len(data):
        raise ValueError("Truncated PE optional header or section table")
    if struct.unpack_from("<H", data, optional)[0] != 0x020B:
        raise ValueError("Expected a PE32+ image")
    if struct.unpack_from("<H", data, optional + 68)[0] != 2:
        raise ValueError("Expected a Windows GUI executable")
    directories = struct.unpack_from("<I", data, optional + 108)[0]
    if directories < 5 or directories > (optional_size - 112) // 8:
        raise ValueError("Invalid PE data-directory count")
    certificate_offset, certificate_size = struct.unpack_from("<II", data, optional + 144)
    if certificate_offset or certificate_size:
        raise ValueError("Signed or certificate-bearing EXEs are not allowed in this unsigned release")
    for section in range(sections):
        header = optional + optional_size + section * 40
        raw_size, raw_offset = struct.unpack_from("<II", data, header + 16)
        if raw_size and (raw_offset == 0 or raw_offset + raw_size > len(data)):
            raise ValueError("Invalid PE section data range")
    return {"machine": "AMD64", "format": "PE32+", "subsystem": "Windows GUI", "certificate_table_bytes": 0}


def checksums(entries: list[Entry]) -> bytes:
    return "".join(f"{entry.sha256}  {entry.name}\n" for entry in entries).encode("utf-8")


def portable_entries(binary: Entry, sources: list[Entry]) -> list[Entry]:
    """Map only runtime files and complete third-party license texts."""
    by_name = {entry.name: entry for entry in sources}
    entries = [binary]
    for archive_name, source_name in PORTABLE_FILES.items():
        if source_name not in by_name:
            raise ValueError(f"Required portable file is missing: {source_name}")
        entries.append(Entry(archive_name, by_name[source_name].data))
    license_prefix = "third_party/licenses/"
    licenses = [entry for entry in sources if entry.name.startswith(license_prefix)]
    if not licenses:
        raise ValueError("The third-party license tree is empty or missing")
    entries.extend(Entry("licenses/" + entry.name[len(license_prefix):], entry.data) for entry in licenses)
    if len({entry.name.casefold() for entry in entries}) != len(entries):
        raise ValueError("Portable archive paths collide on Windows")
    return entries


def write_zip(path: Path, entries: list[Entry]) -> None:
    if len({entry.name.casefold() for entry in entries}) != len(entries):
        raise ValueError("Archive paths collide on Windows")
    with zipfile.ZipFile(path, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for entry in entries:
            info = zipfile.ZipInfo(entry.name, date_time=ZIP_TIMESTAMP)
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, entry.data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    with zipfile.ZipFile(path) as archive:
        bad = archive.testzip()
        if bad is not None or archive.namelist() != [entry.name for entry in entries]:
            raise ValueError(f"Archive verification failed: {path.name}")
        for entry in entries:
            if hashlib.sha256(archive.read(entry.name)).hexdigest() != entry.sha256:
                raise ValueError(f"Archive source differs: {entry.name}")


def package(root: Path, executable: Path, output: Path, check_only: bool = False) -> dict:
    root = root.resolve()
    output = output.resolve()
    if any(output.is_relative_to((root / directory).resolve()) for directory in SOURCE_DIRECTORIES):
        raise ValueError("--output must be outside the allowlisted source directories")
    if is_link(executable) or not executable.is_file():
        raise ValueError("--exe must name a regular file, not a link")
    binary = Entry("nanoDNS.exe", executable.read_bytes())
    pe = validate_unsigned_pe(binary.data)
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", version):
        raise ValueError("Cargo.toml has an unsafe or unsupported package version")
    sources = source_entries(root)
    portable = portable_entries(binary, sources)
    source = [*sources, Entry("SHA256SUMS.txt", checksums(sources))]
    prefix = f"nanoDNS-Desktop-{version}"
    portable_name = f"{prefix}-windows-x64.zip"
    source_name = f"{prefix}-source.zip"
    manifest = {
        "schema": 1, "version": version, "platform": "windows-x64", "signed": False,
        "pe": pe, "executable": {"path": binary.name, "bytes": len(binary.data), "sha256": binary.sha256},
        "source_archive": source_name, "source_bundled_in_portable": False,
        "portable_files": [{"path": entry.name, "bytes": len(entry.data), "sha256": entry.sha256} for entry in portable],
        "source_files": [{"path": entry.name, "bytes": len(entry.data), "sha256": entry.sha256} for entry in sources],
        "archives": [],
    }
    if check_only:
        return {"unsigned_pe": "PASS", "version": version, "source_files": len(sources), "source_archive": source_name, "source_bundled_in_portable": False, "executable_sha256": binary.sha256}
    output.mkdir(parents=True, exist_ok=True)
    final_names = (portable_name, source_name, "release-manifest.json", "SHA256SUMS.txt")
    for name in final_names:
        if (output / name).exists() or (output / name).is_symlink():
            raise ValueError(f"Refusing to overwrite existing release artifact: {name}; choose a fresh --output directory")
    with tempfile.TemporaryDirectory(prefix=".nanodns-package-", dir=output) as staging_name:
        staging = Path(staging_name).resolve()
        if staging.parent != output:
            raise ValueError("Package staging directory is outside the selected output directory")
        write_zip(staging / portable_name, portable)
        write_zip(staging / source_name, source)
        archive_entries = [Entry(name, (staging / name).read_bytes()) for name in (portable_name, source_name)]
        manifest["archives"] = [{"path": entry.name, "bytes": len(entry.data), "sha256": entry.sha256} for entry in archive_entries]
        manifest_entry = Entry("release-manifest.json", (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
        (staging / manifest_entry.name).write_bytes(manifest_entry.data)
        (staging / "SHA256SUMS.txt").write_bytes(checksums(archive_entries))
        # Same-directory hard links commit complete files without overwriting a
        # concurrent writer. TemporaryDirectory then removes only our staging.
        for name in final_names:
            os.link(staging / name, output / name)
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True, help="Exact unsigned Windows x64 GUI EXE to package")
    parser.add_argument("--output", type=Path, default=ROOT / "dist", help="Fresh output directory; existing artifacts are never overwritten")
    parser.add_argument("--check-only", action="store_true", help="Validate unsigned PE and allowlisted source inputs without writing artifacts")
    args = parser.parse_args()
    try:
        result = package(ROOT, args.exe.absolute(), args.output, args.check_only)
    except (OSError, ValueError, KeyError, tomllib.TOMLDecodeError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Packaging failed: {error}\n")
    summary = result if args.check_only else {
        "version": result["version"], "signed": result["signed"],
        "executable": result["executable"], "source_files": len(result["source_files"]),
        "archives": result["archives"],
    }
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
