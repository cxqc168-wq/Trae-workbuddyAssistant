"""Prepare GitHub-safe filenames from installers that passed payload verification."""
import hashlib
import json
from pathlib import Path
import re
import shutil

ROOT = Path(__file__).resolve().parents[1]


def digest(file: Path) -> str:
    with file.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main() -> None:
    release = ROOT / "release"
    report = json.loads((release / "installer-verification.json").read_text(encoding="utf-8"))
    config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    version = report["version"]
    if version != config["version"] or not re.fullmatch(r"[0-9A-Za-z.+-]+", version):
        raise RuntimeError("Installer verification version does not match the project")
    if len(report["installers"]) != 2:
        raise RuntimeError("Both MSI and EXE must pass payload verification")
    upload = release / "upload"
    upload.mkdir(exist_ok=True)
    kinds = set()
    published = []
    for installer in report["installers"]:
        name = installer["installer"]
        if Path(name).name != name:
            raise RuntimeError("Unsafe installer filename")
        extension = Path(name).suffix
        if extension not in (".msi", ".exe") or installer["runtime_smoke_test"] != "passed":
            raise RuntimeError("Installer has not passed payload verification")
        kind = "msi" if extension == ".msi" else "nsis"
        kinds.add(kind)
        source = ROOT / f"src-tauri/target/release/bundle/{kind}" / name
        if digest(source) != installer["sha256"]:
            raise RuntimeError(f"Verified installer changed: {name}")
        suffix = "x64_zh-CN.msi" if extension == ".msi" else "x64-setup.exe"
        asset = upload / f"TraeWorkAssistant_{version}_{suffix}"
        shutil.copy2(source, asset)
        installer["release_asset"] = asset.name
        published.append(asset)
    if kinds != {"msi", "nsis"}:
        raise RuntimeError("Verification report must contain one MSI and one EXE")
    report["verification_method"] = "extracted-installer-payload"
    (release / "installer-verification.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    for name in ("runtime-manifest.json", "installer-verification.json"):
        shutil.copy2(release / name, upload / name)
        published.append(upload / name)
    sums = "".join(f"{digest(file)}  {file.name}\n" for file in published)
    (upload / "SHA256SUMS.txt").write_text(sums, encoding="utf-8")
    published.append(upload / "SHA256SUMS.txt")
    (upload / "assets.json").write_text(json.dumps([
        {"name": file.name, "bytes": file.stat().st_size, "sha256": digest(file)}
        for file in published], indent=2) + "\n", encoding="utf-8")
    print(f"Prepared {len(published)} verified release assets in release/upload", flush=True)


if __name__ == "__main__":
    main()
