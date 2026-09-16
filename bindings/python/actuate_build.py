"""PEP 517 backend for the release sdist. Fetches a verified wheel, never builds Rust."""
from __future__ import annotations
import hashlib
import io
import json
import os
from pathlib import Path
import re
import tarfile
import tempfile
import urllib.request
import zipfile
from packaging.tags import sys_tags
from packaging.utils import parse_wheel_filename
try:
    import tomllib
except ImportError:
    import tomli as tomllib

ROOT = Path(__file__).parent
REPOSITORY = "Rajaniraiyn/actuate"

def version() -> str:
    value = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[a-zA-Z0-9.-]+)?", value):
        raise ValueError("Invalid package version")
    return value

def _read(url: str, limit: int, headers: dict[str, str] | None = None) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "actuate-installer", **(headers or {})})
    # urllib forwards custom authorization on redirects. Strip it on asset redirects.
    class Redirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, request, fp, code, msg, headers, newurl):
            redirected = super().redirect_request(request, fp, code, msg, headers, newurl)
            if redirected is not None:
                redirected.remove_header("Authorization")
            return redirected
    with urllib.request.build_opener(Redirect).open(request, timeout=120) as response:
        data = response.read(limit + 1)
    if len(data) > limit: raise RuntimeError("Release download exceeds expected size")
    return data

def downloader(release: str):
    mirror = os.environ.get("ACTUATE_RELEASE_BASE_URL")
    if mirror and not mirror.startswith("https://"):
        raise RuntimeError("Release mirrors must use HTTPS")
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    assets = None
    if token and not mirror:
        data = _read(f"https://api.github.com/repos/{REPOSITORY}/releases/tags/v{release}", 4 * 1024 * 1024,
                     {"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json"})
        assets = {asset["name"]: asset["url"] for asset in json.loads(data)["assets"]}
    def download(name: str, limit: int) -> bytes:
        if assets is not None:
            return _read(assets[name], limit, {"Authorization": f"Bearer {token}", "Accept": "application/octet-stream"})
        return _read(f"{mirror or f'https://github.com/{REPOSITORY}/releases/download'}/v{release}/{name}", limit)
    return download

def select_wheel(manifest: dict, release: str, tags=None) -> str:
    ranks = {tag: index for index, tag in enumerate(sys_tags() if tags is None else tags)}
    candidates = []
    for name in manifest["assets"]:
        if not name.endswith(".whl") or Path(name).name != name: continue
        distribution, wheel_version, _, wheel_tags = parse_wheel_filename(name)
        if distribution != "actuate" or str(wheel_version) != release: continue
        matches = [ranks[tag] for tag in wheel_tags if tag in ranks]
        if matches: candidates.append((min(matches), name))
    if not candidates:
        raise RuntimeError("No compatible Actuate release wheel for this Python, OS, and architecture. Build from the repository with maturin.")
    return min(candidates)[1]

def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    release = version()
    download = downloader(release)
    manifest = json.loads(download("manifest.json", 1024 * 1024))
    if manifest.get("version") != release: raise RuntimeError("Release version mismatch")
    name = select_wheel(manifest, release)
    entry = manifest["assets"][name]
    size, checksum = entry["size"], entry["sha256"]
    if not isinstance(size, int) or not 0 < size <= 256 * 1024 * 1024 or not re.fullmatch(r"[a-f0-9]{64}", checksum):
        raise RuntimeError("Invalid release asset metadata")
    data = download(name, size)
    if len(data) != size or hashlib.sha256(data).hexdigest() != checksum:
        raise RuntimeError("Wheel checksum mismatch")
    with zipfile.ZipFile(io.BytesIO(data)) as wheel:
        if wheel.testzip() is not None: raise RuntimeError("Invalid wheel archive")
    directory = Path(wheel_directory); directory.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=directory, delete=False) as output:
        temporary = Path(output.name); output.write(data)
    try: os.replace(temporary, directory / name)
    finally: temporary.unlink(missing_ok=True)
    return name

def get_requires_for_build_wheel(config_settings=None): return []
def get_requires_for_build_sdist(config_settings=None): return []

def build_sdist(sdist_directory, config_settings=None):
    release = version()
    name = f"actuate-{release}.tar.gz"
    directory = Path(sdist_directory); directory.mkdir(parents=True, exist_ok=True)
    prefix = f"actuate-{release}"
    with tarfile.open(directory / name, "w:gz") as archive:
        for path in [ROOT / "pyproject.toml", ROOT / "actuate_build.py", ROOT / "README.md", *sorted((ROOT / "actuate").rglob("*"))]:
            if path.is_file() and path.suffix not in {".so", ".pyd", ".pyc"} and "__pycache__" not in path.parts:
                archive.add(path, arcname=f"{prefix}/{path.relative_to(ROOT).as_posix()}")
        metadata = f"Metadata-Version: 2.4\nName: actuate\nVersion: {release}\nRequires-Python: >=3.10\n".encode()
        info = tarfile.TarInfo(f"{prefix}/PKG-INFO"); info.size = len(metadata)
        archive.addfile(info, io.BytesIO(metadata))
    return name
