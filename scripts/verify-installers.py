"""Extract both installers, match runtime hashes, and test their actual payloads."""
import argparse
from contextlib import nullcontext
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
NS = {"w": "http://schemas.microsoft.com/wix/2006/wi"}


def digest(file: Path) -> str:
    with file.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def extract_msi(installer: Path, stage: Path, dark: Path) -> tuple[Path, Path]:
    raw = stage / "raw"
    xml = stage / "package.wxs"
    subprocess.run([str(dark), "-nologo", "-x", str(raw), str(installer), str(xml)],
                   capture_output=True, text=True, errors="replace", check=True, timeout=300)
    tree = ET.parse(xml).getroot()
    install = tree.find(".//w:Directory[@Id='INSTALLDIR']", NS)
    if install is None:
        raise RuntimeError("MSI has no application installation directory")
    payload = stage / "payload"
    payload.mkdir()

    def visit(node: ET.Element, directory: Path) -> None:
        for child in node:
            tag = child.tag.rsplit("}", 1)[-1]
            if tag == "Directory":
                visit(child, directory / child.attrib["Name"])
            elif tag == "File":
                target = directory / child.attrib["Name"]
                if not target.resolve().is_relative_to(payload.resolve()):
                    raise RuntimeError(f"Unsafe MSI file path: {target}")
                target.parent.mkdir(parents=True, exist_ok=True)
                source = Path(child.attrib["Source"])
                os.link(source, target)
            else:
                visit(child, directory)

    visit(install, payload)
    webviews = list((raw / "Binary").glob("*WebView2*"))
    if len(webviews) != 1:
        raise RuntimeError("MSI does not include exactly one offline WebView2 installer")
    return payload, webviews[0]


def extract_exe(installer: Path, stage: Path, sevenzip: Path) -> tuple[Path, Path]:
    subprocess.run([str(sevenzip), "x", "-y", f"-o{stage}", str(installer)],
                   capture_output=True, text=True, errors="replace", check=True, timeout=300)
    python = list(stage.rglob("python.exe"))
    if len(python) != 1:
        raise RuntimeError("EXE does not contain exactly one bundled Python runtime")
    payload = python[0].parent.parent
    webviews = [file for file in stage.rglob("*WebView2*.exe") if file.is_file()]
    if len(webviews) != 1:
        raise RuntimeError("EXE does not include exactly one offline WebView2 installer")
    return payload, webviews[0]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sevenzip", type=Path,
                        default=Path(shutil.which("7z") or r"C:\Program Files\7-Zip\7z.exe"))
    parser.add_argument("--dark", type=Path,
                        default=Path(os.environ["LOCALAPPDATA"]) / "tauri/WixTools314/dark.exe")
    parser.add_argument("--output", type=Path, default=ROOT / "release/installer-verification.json")
    parser.add_argument("--keep-payloads", action="store_true", help="Retain extracted files for local UI testing")
    parser.add_argument("--kind", choices=("msi", "nsis"), help="Check one installer while the other is still building")
    args = parser.parse_args()
    config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    version = config["version"]
    manifest = json.loads((ROOT / "release/runtime-manifest.json").read_text(encoding="utf-8"))
    module = importlib.util.spec_from_file_location("verify_runtime", ROOT / "scripts/verify-release-runtime.py")
    verifier = importlib.util.module_from_spec(module)
    module.loader.exec_module(verifier)
    results = []
    for kind in ((args.kind,) if args.kind else ("msi", "nsis")):
        extension = "msi" if kind == "msi" else "exe"
        matches = list((ROOT / f"src-tauri/target/release/bundle/{kind}").glob(f"*_{version}_*.{extension}"))
        if len(matches) != 1:
            raise RuntimeError(f"Expected one {kind} installer for {version}: {matches}")
        installer = matches[0]
        print(f"Inspecting {installer.name}", flush=True)
        staging = (nullcontext(tempfile.mkdtemp(prefix=f"traework-{kind}-verify-")) if args.keep_payloads
                   else tempfile.TemporaryDirectory(prefix=f"traework-{kind}-verify-"))
        with staging as tmp:
            stage = Path(tmp)
            payload, webview = (extract_msi(installer, stage, args.dark) if kind == "msi"
                                else extract_exe(installer, stage, args.sevenzip))
            if webview.stat().st_size < 50_000_000:
                raise RuntimeError("WebView2 resource is a bootstrapper, not a complete offline installer")
            for relative, expected in manifest["files"].items():
                file = payload / relative
                if not file.is_file() or file.stat().st_size != expected["bytes"] or digest(file) != expected["sha256"]:
                    raise RuntimeError(f"{kind} runtime resource mismatch: {relative}")
            app = payload / "trae-work-assistant.exe"
            if not app.is_file() or app.stat().st_size < 1_000_000:
                raise RuntimeError(f"{kind} application executable missing")
            print(f"{kind}: {len(manifest['files'])} resource hashes match; offline WebView2 present", flush=True)
            verifier.verify(payload)
            if args.keep_payloads:
                print(f"Retained {kind} payload: {payload}", flush=True)
            results.append({"installer": installer.name, "bytes": installer.stat().st_size,
                            "sha256": digest(installer), "verified_resource_files": len(manifest["files"]),
                            "webview2_bytes": webview.stat().st_size, "webview2_sha256": digest(webview),
                            "runtime_smoke_test": "passed"})
    args.output.write_text(json.dumps({"version": version, "installers": results}, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Verified {len(results)} installer payload(s).", flush=True)


if __name__ == "__main__":
    main()
