"""Stage complete, pinned Windows x64 runtime resources for Tauri installers."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "release" / "cache"
RESOURCES = ROOT / "resources"
PYTHON_VERSION = "3.13.16"
PYTHON_SHA256 = "bbf675bb5e763c1efbb09a3a461b259d81598a63c30c4b0d7ea11b9f063df159"
CHROME_VERSION = "154.0.8037.92"
CHROME_SHA256 = "b897ef3601c947ac0620c784556dec719ac602b0159ce105927acf645ee0f598"
REQUIREMENTS = ROOT / "src-python" / "requirements-release.txt"


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def download(url: str, name: str, expected: str | None = None) -> Path:
    CACHE.mkdir(parents=True, exist_ok=True)
    dest = CACHE / name
    if not dest.exists():
        print(f"Downloading {url}", flush=True)
        temporary = dest.with_suffix(".download")
        with urllib.request.urlopen(url, timeout=120) as source, temporary.open("wb") as output:
            shutil.copyfileobj(source, output)
        temporary.replace(dest)
    if expected and sha256(dest) != expected:
        raise RuntimeError(f"SHA256 mismatch: {dest}; remove the corrupt cached file and retry")
    return dest


def extract(archive: Path, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    base = destination.resolve()
    with zipfile.ZipFile(archive) as bundle:
        for info in bundle.infolist():
            if not (base / info.filename).resolve().is_relative_to(base):
                raise RuntimeError(f"Unsafe archive path: {info.filename}")
        bundle.extractall(base)


def main() -> None:
    if os.name != "nt":
        raise RuntimeError("Windows x64 release preparation must run on Windows")
    python_dir = RESOURCES / "python"
    marker = python_dir / ".runtime-ready.json"
    expected = {"python": PYTHON_VERSION, "python_sha256": PYTHON_SHA256,
                "requirements_sha256": sha256(REQUIREMENTS)}
    ready = json.loads(marker.read_text()) if marker.exists() else None
    if ready != expected:
        archive = download(
            f"https://www.python.org/ftp/python/{PYTHON_VERSION}/python-{PYTHON_VERSION}-amd64.zip",
            f"python-{PYTHON_VERSION}-amd64.zip", PYTHON_SHA256)
        extract(archive, python_dir)
        site = python_dir / "Lib" / "site-packages"
        subprocess.run([sys.executable, "-m", "pip", "--isolated", "install", "--only-binary=:all:", "--no-deps", "--require-hashes",
                        "--python-version", "3.13", "--platform", "win_amd64",
                        "--implementation", "cp", "--abi", "cp313", "--upgrade",
                        "--target", str(site), "--no-compile", "-r", str(REQUIREMENTS)], check=True)
        # pywin32 DLLs must be discoverable in a relocated runtime without its installer.
        for dll in (site / "pywin32_system32").glob("*.dll"):
            shutil.copy2(dll, python_dir / dll.name)
        marker.write_text(json.dumps(expected), encoding="utf-8")
    for script in (ROOT / "src-python").glob("*.py"):
        shutil.copy2(script, python_dir / script.name)
    shutil.copy2(REQUIREMENTS, python_dir / REQUIREMENTS.name)
    browser_dir = RESOURCES / "browser"
    browser_marker = browser_dir / ".runtime-ready.json"
    browser_expected = {"chrome": CHROME_VERSION, "chrome_sha256": CHROME_SHA256}
    if not browser_marker.exists() or json.loads(browser_marker.read_text()) != browser_expected:
        archive = download(
            f"https://storage.googleapis.com/chrome-for-testing-public/{CHROME_VERSION}/win64/chrome-win64.zip",
            f"chrome-{CHROME_VERSION}-win64.zip", CHROME_SHA256)
        extract(archive, browser_dir)
        browser_marker.write_text(json.dumps(browser_expected), encoding="utf-8")
    ps_dir = RESOURCES / "ps"
    ps_dir.mkdir(parents=True, exist_ok=True)
    for script in (ROOT / "src-ps").glob("*.ps1"):
        shutil.copy2(script, ps_dir / script.name)
    subprocess.run([sys.executable, str(ROOT / "scripts" / "verify-release-runtime.py")], check=True)
    files = {}
    for folder in (python_dir, browser_dir, ps_dir):
        for file in sorted(folder.rglob("*")):
            if file.is_file() and "__pycache__" not in file.parts:
                files[file.relative_to(RESOURCES).as_posix()] = {"bytes": file.stat().st_size, "sha256": sha256(file)}
    manifest = {"architecture": "windows-x64", **expected, **browser_expected,
                "license_guard_public_key_sha256": sha256(ROOT / "src-tauri/src/license_guard/public_key.pem"),
                "packages": REQUIREMENTS.read_text().splitlines()[1:], "files": files}
    (ROOT / "release" / "runtime-manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Release runtime ready: {len(files)} files; manifest: release/runtime-manifest.json", flush=True)


if __name__ == "__main__":
    main()
