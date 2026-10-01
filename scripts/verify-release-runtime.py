"""Smoke-test installed resources without relying on host Python/browser packages."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def verify(root: Path) -> None:
    python = root / "python" / "python.exe"
    chrome = root / "browser" / "chrome-win64" / "chrome.exe"
    for file in (python, chrome, root / "python" / "device_proxy.py",
                 root / "python" / "auto_checkin.py", root / "ps" / "trae-switch-bridge.ps1"):
        if not file.is_file():
            raise RuntimeError(f"Missing required release resource: {file}")
    env = os.environ.copy()
    for key in ("VIRTUAL_ENV", "ELECTRON_RUN_AS_NODE"):
        env.pop(key, None)
    windows_dir = env.get("SYSTEMROOT") or env.get("SystemRoot") or env.get("WINDIR")
    if not windows_dir:
        raise RuntimeError("Windows system directory is unavailable")
    env.update(PATH=str(Path(windows_dir) / "System32"),
               PYTHONHOME=str(root / "nonexistent-host-python"),
               PYTHONPATH=str(root / "nonexistent-host-site-packages"))
    # -I also disables all user site-packages and Python environment overrides.
    probe = r'''
import sys, pathlib, ssl, sqlite3, ctypes, json
import cryptography, cffi, brotli, win32crypt
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
base = pathlib.Path(sys.executable).parent.resolve()
for module in (ssl, sqlite3, cryptography, cffi, brotli, win32crypt):
    assert pathlib.Path(module.__file__).resolve().is_relative_to(base), module.__file__
data = b"release-runtime-probe"
assert brotli.decompress(brotli.compress(data)) == data
blob = win32crypt.CryptProtectData(data, None, None, None, None, 0)
assert win32crypt.CryptUnprotectData(blob, None, None, None, 0)[1] == data
aes = AESGCM(AESGCM.generate_key(bit_length=256))
nonce = bytes(12)
assert aes.decrypt(nonce, aes.encrypt(nonce, data, None), None) == data
assert sqlite3.connect(":memory:").execute("select 1").fetchone() == (1,)
ssl.create_default_context()
print(json.dumps({"python": sys.version.split()[0], "cryptography": cryptography.__version__, "ssl": ssl.OPENSSL_VERSION}))
'''
    result = subprocess.run([str(python), "-I", "-X", "utf8", "-c", probe], env=env,
                            text=True, capture_output=True, timeout=60, check=True)
    print(result.stdout.strip(), flush=True)
    with tempfile.TemporaryDirectory(prefix="traework-release-check-") as tmp:
        env["TRAEDATA_DIR"] = tmp
        subprocess.run([str(python), "-I", "-X", "utf8", str(python.parent / "device_proxy.py"), "--gen-ca"],
                       env=env, capture_output=True, timeout=60, check=True)
        for name in ("ca.crt", "ca.key", "ca.cer"):
            assert (Path(tmp) / "data" / "certs" / name).is_file(), name
        profile = Path(tmp) / "chrome-profile"
        # CDP readiness tests the same interface the desktop application uses.
        # Windows GUI Chrome does not reliably close inherited dump-dom pipes.
        with (Path(tmp) / "chrome.log").open("wb") as log:
            browser = subprocess.Popen([
                str(chrome), "--headless=new", "--disable-gpu", "--no-sandbox",
                "--no-first-run", "--no-default-browser-check", "--disable-background-networking",
                f"--user-data-dir={profile}", "--remote-debugging-port=0",
                "data:text/html,<title>TraeWorkRuntimeOK</title>"],
                env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 30
                port_file = profile / "DevToolsActivePort"
                opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
                while time.monotonic() < deadline:
                    if browser.poll() is not None:
                        raise RuntimeError(f"Bundled browser exited with {browser.returncode}")
                    if port_file.exists():
                        port = int(port_file.read_text().splitlines()[0])
                        with opener.open(f"http://127.0.0.1:{port}/json", timeout=5) as response:
                            tabs = json.load(response)
                        if any(tab.get("title") == "TraeWorkRuntimeOK" for tab in tabs):
                            break
                    time.sleep(0.2)
                else:
                    raise RuntimeError("Bundled browser CDP did not load the test page")
            finally:
                if browser.poll() is None:
                    subprocess.run([str(Path(windows_dir) / "System32" / "taskkill.exe"),
                                    "/PID", str(browser.pid), "/T", "/F"],
                                   capture_output=True, timeout=15, check=True)
                browser.wait(timeout=15)
    print("Bundled runtime checks passed (isolated Python, DPAPI, TLS, SQLite, proxy CA, browser).", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--resources", type=Path, default=ROOT / "resources")
    args = parser.parse_args()
    verify(args.resources.resolve())
