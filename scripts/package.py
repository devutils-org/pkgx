#!/usr/bin/env python3
"""Build a deterministic, reviewable archive. Never install or execute its binary."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def regular_bytes(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"expected a regular file: {path.name}")
    return path.read_bytes()


def publish(path, data):
    """Publish without overwriting an existing artifact or following its symlink."""
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".xpac-", delete=False) as temp:
        staging = Path(temp.name)
        try:
            temp.write(data)
            temp.flush()
            os.fsync(temp.fileno())
            os.chmod(staging, 0o644)
            os.link(staging, path)
        finally:
            staging.unlink(missing_ok=True)


def package(binary, target, output_dir, epoch, rustc_info=None):
    if not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}", target):
        raise ValueError("invalid target identifier")
    if not 0 <= epoch <= 0xFFFFFFFF:
        raise ValueError("epoch must be between 0 and 4294967295")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][a-zA-Z0-9.-]+)?", version):
        raise ValueError("invalid workspace version")
    binary_bytes = regular_bytes(binary)
    compiler = regular_bytes(rustc_info).decode("utf-8") if rustc_info else None
    if compiler is not None and len(compiler) > 16384:
        raise ValueError("compiler information exceeds 16 KiB")
    root_name = f"xpac-v{version}-{target}"
    manifest = {
        "schema_version": 1,
        "version": version,
        "target": target,
        "source_date_epoch": epoch,
        "binary_sha256": sha256(binary_bytes),
        "cargo_lock_sha256": sha256(regular_bytes(ROOT / "Cargo.lock")),
        "rustc_verbose_version": compiler,
        "native_certification": False,
    }
    files = {
        "xpac": (binary_bytes, 0o755),
        "BUILD.json": ((json.dumps(manifest, sort_keys=True, indent=2) + "\n").encode(), 0o644),
    }
    names = ["README.md", "REFERENCE.md", "CHANGELOG.md", "LICENSE",
             "LICENSE-MIT", "LICENSE-APACHE", "xpac.md",
             "completions/xpac.bash", "completions/_xpac", "completions/xpac.fish"]
    names += [str(path.relative_to(ROOT)) for path in sorted((ROOT / "docs").rglob("*.md"))]
    names += [str(path.relative_to(ROOT)) for path in sorted((ROOT / "examples").glob("*.toml"))]
    for name in names:
        files[name] = (regular_bytes(ROOT / name), 0o644)
    # Normalize ordering, owners, timestamps and gzip headers. Equal inputs produce
    # equal archives; the Rust compiler/linker inputs themselves are not normalized.
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, mode="wb", filename="", mtime=epoch) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
            for name, (data, mode) in sorted(files.items()):
                entry = tarfile.TarInfo(f"{root_name}/{name}")
                entry.size = len(data)
                entry.mode = mode
                entry.mtime = epoch
                entry.uid = entry.gid = 0
                entry.uname = entry.gname = "root"
                archive.addfile(entry, io.BytesIO(data))
    output_dir.mkdir(parents=True, exist_ok=True)
    if output_dir.is_symlink() or not output_dir.is_dir():
        raise ValueError("output directory must be a real directory")
    path = output_dir / f"{root_name}.tar.gz"
    checksum_path = path.with_name(path.name + ".sha256")
    # Refuse both preexisting destinations before creating either artifact.
    if os.path.lexists(path) or os.path.lexists(checksum_path):
        raise FileExistsError("artifact or checksum already exists")
    archive_bytes = buffer.getvalue()
    publish(path, archive_bytes)
    publish(checksum_path, f"{sha256(archive_bytes)}  {path.name}\n".encode())
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    parser.add_argument("--epoch", type=int, default=os.environ.get("SOURCE_DATE_EPOCH", "0"))
    parser.add_argument("--rustc-info", type=Path)
    args = parser.parse_args()
    try:
        path = package(args.binary, args.target, args.output_dir, args.epoch, args.rustc_info)
    except (ValueError, OSError, tomllib.TOMLDecodeError) as error:
        parser.exit(1, f"packaging failed: {error}\n")
    print(path)


if __name__ == "__main__":
    main()
