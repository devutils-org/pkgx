#!/usr/bin/env python3
"""Package exactly the two Linux builds used by the VM matrix."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("xpac_package", ROOT / "scripts/package.py")
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


def glibc_floor(binary):
    result = subprocess.run(["readelf", "--version-info", str(binary)],
                            check=True, stdout=subprocess.PIPE, universal_newlines=True)
    names = set(re.findall(r"GLIBC_([A-Za-z0-9_.]+)", result.stdout))
    if any(not re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", name) for name in names):
        raise ValueError("GNU binary has an unknown or private GLIBC ABI requirement")
    versions = {tuple(map(int, v.split('.'))) for v in names}
    if not versions or max(versions) > (2, 17):
        raise ValueError("GNU binary needs symbols above glibc 2.17 or its floor is unknown")


def main():
    build = Path(os.environ.get("CARGO_TARGET_DIR", "/tmp/xpac-target"))
    payload = ROOT / "dist/distroproof"
    payload.mkdir(parents=True, exist_ok=True)
    compiler = subprocess.check_output(["rustc", "-vV"]).decode()
    artifacts = {}
    with tempfile.TemporaryDirectory(prefix="xpac-matrix-package-") as directory:
        temp = Path(directory)
        rustc = temp / "rustc.txt"
        rustc.write_text(compiler)
        for flavor in ("gnu", "musl"):
            target = "x86_64-unknown-linux-" + flavor
            binary = build / target / "release/xpac"
            if flavor == "gnu":
                glibc_floor(binary)
            else:
                program = subprocess.check_output(["readelf", "-l", str(binary)]).decode()
                dynamic = subprocess.check_output(["readelf", "-d", str(binary)]).decode()
                if "INTERP" in program or "(NEEDED)" in dynamic:
                    raise ValueError("musl binary must be static without a dynamic interpreter")
            archive = packaging.package(binary, target, temp / flavor, 0, rustc)
            for source in (archive, archive.with_name(archive.name + ".sha256")):
                (payload / source.name).write_bytes(source.read_bytes())
            # Stable name in the payload prevents stale version glob selection.
            stable = payload / (flavor + ".tar.gz")
            stable.write_bytes(archive.read_bytes())
            digest = hashlib.sha256(stable.read_bytes()).hexdigest()
            stable.with_name(stable.name + ".sha256").write_text(digest + "  " + stable.name + "\n")
            artifacts[flavor] = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                                 "archive_sha256": digest, "target": target}
    (payload / "builds.json").write_text(json.dumps(artifacts, indent=2) + "\n")
    print(payload / "builds.json")


if __name__ == "__main__":
    main()
