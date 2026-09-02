#!/usr/bin/env python3
"""快速检查所有账号的 ide_credits vs work_credits"""
import json, os, ssl, urllib.request, time, hashlib, re

APP_DATA = os.path.join(os.environ.get("APPDATA", ""), "TraeWorkAssistant", "data")
accounts = json.load(open(os.path.join(APP_DATA, "checkin_accounts.json"), "r", encoding="utf-8"))["accounts"]
device_map = json.load(open(os.path.join(APP_DATA, "device_map.json"), "r", encoding="utf-8"))

EP_CHAT = "/api/agent/v3/llm_utils_chat"
APP_ID = "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8"
HOST = "https://trae-api-cn.mchost.guru"

ctx = ssl.create_default_context()
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE

body = json.dumps({
    "max_tokens": 10,
    "messages": [{"content": [{"text": "hi", "type": "text"}], "role": "user"}],
    "stream": True,
    "config_name": "glm-5.2",
    "function": "solo_work_lite",
    "model": "glm-5.2",
}).encode()

os.environ["NO_PROXY"] = "*"

print(f"{'Account':<14} {'ide_credits':>12} {'work_credits':>14} {'Result':<10}")
print("-" * 56)

for acc in accounts:
    name = acc["name"]
    uid = acc["UserID"]
    jwt = acc["jwt"]
    if jwt.startswith("Cloud-IDE-JWT "):
        jwt = jwt[len("Cloud-IDE-JWT "):]
    dev = device_map.get(uid, {})
    dev_id = dev.get("device_id", "199439841787403")
    mach_id = hashlib.sha256(f"mach:{uid}".encode()).hexdigest()

    headers = {
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
        "User-Agent": "Trae/0.1.50",
        "Authorization": f"Cloud-IDE-JWT {jwt}",
        "X-Cloudide-Token": jwt,
        "X-Ide-Token": jwt,
        "X-Uid": uid,
        "X-App-Id": APP_ID,
        "X-App-Version": "default",
        "X-Ide-Version": "0.1.50",
        "X-Ide-Version-Code": "20260811",
        "X-App-Version-Code": "20260811",
        "X-Ide-Version-Type": "stable",
        "X-Device-Type": "windows",
        "X-OS-Version": "Windows 11 Home China",
        "X-Device-Brand": "CREFG-XX",
        "Request-Traffic-Type": "prod",
        "X-Machine-Id": mach_id,
        "X-Device-Id": dev_id,
        "Referer": f"https://trae-api-cn.mchost.guru{EP_CHAT}",
    }

    req = urllib.request.Request(HOST + EP_CHAT, data=body, headers=headers, method="POST")
    try:
        resp = urllib.request.urlopen(req, timeout=15, context=ctx)
        raw = resp.read(8192).decode("utf-8", errors="replace")
        ide_m = re.search(r'"ide_credits":([\d.]+)', raw)
        work_m = re.search(r'"work_credits":([\d.]+)', raw)
        ide = float(ide_m.group(1)) if ide_m else -1
        work = float(work_m.group(1)) if work_m else -1
        has_output = "event:output" in raw
        has_quota = "exceeded the quota" in raw.lower()
        if has_output:
            status = "OK"
        elif has_quota:
            status = "QUOTA"
        else:
            status = "ERR"
        ide_str = f"{ide:.1f}" if ide >= 0 else "?"
        work_str = f"{work:.1f}" if work >= 0 else "?"
        print(f"{name:<14} {ide_str:>12} {work_str:>14} {status:<10}")
    except Exception as e:
        print(f"{name:<14} {'?':>12} {'?':>14} ERROR: {e}")
    time.sleep(0.5)

print()
print("Done.")
