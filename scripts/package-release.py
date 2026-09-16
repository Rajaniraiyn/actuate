"""Stage host binaries and assemble checksummed GitHub release assets."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def version():
    rust = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    js = json.loads((ROOT / "bindings/typescript/package.json").read_text())["version"]
    python = tomllib.loads((ROOT / "bindings/python/pyproject.toml").read_text())["project"]["version"]
    if not rust == js == python: raise RuntimeError("Rust, TypeScript, and Python versions must match")
    tag = os.environ.get("GITHUB_REF", "")
    if tag.startswith("refs/tags/") and tag != f"refs/tags/v{rust}": raise RuntimeError("Release tag does not match package versions")
    return rust

def stage(target, profile, output):
    release = version()
    build = ROOT / "target" / target / profile
    if not build.exists(): build = ROOT / "target" / profile
    windows = "windows" in target
    library = "actuate_napi.dll" if windows else "libactuate_napi.dylib" if "apple" in target else "libactuate_napi.so"
    python = "_native.dll" if windows else "lib_native.dylib" if "apple" in target else "lib_native.so"
    native = ROOT / "bindings/typescript/native"; native.mkdir(exist_ok=True)
    shutil.copy2(build / library, native / "actuate.node")
    shutil.copy2(build / python, ROOT / "bindings/python/actuate" / ("_native.pyd" if windows else "_native.so"))
    output.mkdir(parents=True, exist_ok=True)
    shutil.copy2(build / library, output / f"actuate-napi-v{release}-{target}.node")
    executable = "actuate.exe" if windows else "actuate"
    archive_name = f"actuate-v{release}-{target}"
    if windows:
        with zipfile.ZipFile(output / f"{archive_name}.zip", "w", zipfile.ZIP_DEFLATED) as archive:
            archive.write(build / executable, executable)
            archive.write(ROOT / "README.md", "README.md")
            archive.write(ROOT / "vendor/README.md", "THIRD-PARTY.md")
    else:
        with tarfile.open(output / f"{archive_name}.tar.gz", "w:gz") as archive:
            archive.add(build / executable, arcname=executable)
            archive.add(ROOT / "README.md", arcname="README.md")
            archive.add(ROOT / "vendor/README.md", arcname="THIRD-PARTY.md")
    # Include upstream licenses alongside binaries.
    for source in (ROOT / "vendor/patches/droidmux-client").glob("LICENSE-*"):
        shutil.copy2(source, output / source.name)

def manifest(output):
    assets = {}
    for path in sorted(output.iterdir()):
        if path.is_file() and path.name not in {"manifest.json", "SHA256SUMS"}:
            assets[path.name] = {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "size": path.stat().st_size}
    (output / "manifest.json").write_text(json.dumps({"version": version(), "assets": assets}, indent=2) + "\n")
    files = [*assets, "manifest.json"]
    (output / "SHA256SUMS").write_text("".join(f"{hashlib.sha256((output / name).read_bytes()).hexdigest()}  {name}\n" for name in files))

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target")
    parser.add_argument("--stage", action="store_true")
    parser.add_argument("--profile", default="release")
    parser.add_argument("--output", type=Path, default=ROOT / "target/release-assets")
    parser.add_argument("--manifest", action="store_true")
    args = parser.parse_args()
    if args.stage and not args.target:
        args.target = next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    if args.target: stage(args.target, args.profile, args.output)
    if args.manifest: manifest(args.output)
