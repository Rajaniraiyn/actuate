import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile
from packaging.tags import Tag
import actuate_build as backend

class Distribution(unittest.TestCase):
    def test_windows_arm64_selects_native_wheel(self):
        arm = "actuate-0.1.0-cp310-abi3-win_arm64.whl"
        x64 = "actuate-0.1.0-cp310-abi3-win_amd64.whl"
        manifest = {"assets": {arm: {}, x64: {}}}
        self.assertEqual(backend.select_wheel(manifest, "0.1.0", [Tag("cp310", "abi3", "win_arm64")]), arm)

    def test_target_selection_rejects_foreign_or_wrong_version_wheels(self):
        name = "actuate-0.1.0-cp310-abi3-win_amd64.whl"
        manifest = {"assets":{name:{},"actuate-9.0.0-cp310-abi3-win_amd64.whl":{}}}
        self.assertEqual(backend.select_wheel(manifest,"0.1.0",[Tag("cp310","abi3","win_amd64")]), name)
        with self.assertRaises(RuntimeError): backend.select_wheel(manifest,"0.1.0",[Tag("cp310","abi3","linux_x86_64")])
    def test_checksum_is_verified_before_writing_wheel(self):
        name = "actuate-0.1.0-cp310-abi3-win_amd64.whl"
        stream = io.BytesIO()
        with zipfile.ZipFile(stream,"w") as archive: archive.writestr("placeholder", "test")
        data = stream.getvalue()
        manifest = dict(version="0.1.0",assets={name:dict(size=len(data),sha256=hashlib.sha256(data).hexdigest())})
        assets = {"manifest.json":json.dumps(manifest).encode(),name:data}
        with tempfile.TemporaryDirectory() as directory, patch.object(backend,"version",return_value="0.1.0"), patch.object(backend,"select_wheel",return_value=name), patch.object(backend,"downloader",return_value=lambda name,limit: assets[name]):
            self.assertEqual(backend.build_wheel(directory), name)
            self.assertEqual((Path(directory)/name).read_bytes(), data)
            assets[name]=b"corrupt"
            with self.assertRaisesRegex(RuntimeError,"checksum mismatch"): backend.build_wheel(directory)
            self.assertEqual((Path(directory)/name).read_bytes(), data)
    def test_sdist_is_portable_and_has_no_compiled_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            name=backend.build_sdist(directory)
            with tarfile.open(Path(directory)/name) as archive:
                names=archive.getnames()
                self.assertTrue(any(n.endswith("/actuate_build.py") for n in names))
                self.assertFalse(any(n.endswith((".so",".pyd",".pyc")) for n in names))
                metadata=archive.extractfile(next(n for n in names if n.endswith("PKG-INFO"))).read()
                self.assertIn(b"Name: actuate\n",metadata)
