import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("xpac_package", Path(__file__).with_name("package.py"))
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class ArchiveTests(unittest.TestCase):
    def test_deterministic_archive_and_verified_contents(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / "binary"
            binary.write_bytes(b"not executed\x00")
            first = packaging.package(binary, "test-target", root / "first", 123)
            second = packaging.package(binary, "test-target", root / "second", 123)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            checksum = first.with_name(first.name + ".sha256").read_text().split()[0]
            self.assertEqual(checksum, hashlib.sha256(first.read_bytes()).hexdigest())
            with tarfile.open(fileobj=io.BytesIO(first.read_bytes())) as archive:
                files = archive.getmembers()
                self.assertTrue(all(f.uid == f.gid == 0 and f.mtime == 123 for f in files))
                executable = next(f for f in files if f.name.endswith("/xpac"))
                self.assertEqual(executable.mode, 0o755)
                self.assertEqual(archive.extractfile(executable).read(), binary.read_bytes())
                manifest = json.load(archive.extractfile(next(f for f in files if f.name.endswith("/BUILD.json"))))
                self.assertEqual(manifest["binary_sha256"], hashlib.sha256(binary.read_bytes()).hexdigest())
                self.assertFalse(manifest["native_certification"])
                for name in ["LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "REFERENCE.md"]:
                    member = next(f for f in files if f.name.endswith("/" + name))
                    self.assertEqual(archive.extractfile(member).read(),
                                     (packaging.ROOT / name).read_bytes())

    def test_refuses_overwrite_symlinks_and_path_traversal(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / "binary"
            binary.write_bytes(b"original")
            first = packaging.package(binary, "test-target", root / "out", 0)
            original = first.read_bytes()
            with self.assertRaises(FileExistsError):
                packaging.package(binary, "test-target", root / "out", 0)
            self.assertEqual(first.read_bytes(), original)
            link = root / "link"
            link.symlink_to(binary)
            with self.assertRaises(ValueError):
                packaging.package(link, "test-target", root / "other", 0)
            for target in ["../escape", "/absolute", "-option", "target/name"]:
                with self.assertRaises(ValueError):
                    packaging.package(binary, target, root / "other", 0)
            with self.assertRaises(ValueError):
                packaging.package(binary, "test-target", root / "other", -1)
            self.assertFalse(list((root / "out").glob(".xpac-*")))


if __name__ == "__main__":
    unittest.main()
