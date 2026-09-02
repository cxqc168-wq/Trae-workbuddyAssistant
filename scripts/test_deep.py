#!/usr/bin/env python3
"""
上游 API 请求测试脚本 — 深度测试不同参数组合，定位 work_credits 使用方法。
"""

import json
import os
import time
import hashlib
import argparse
import urllib.request
import urllib.error
import ssl

APP_DATA = os.path.join(os.environ.get("APPDATA", ""), "TraeWorkAssistant", "data")
ACCOUNTS_FILE = os.path.join(APP_DATA, "checkin_accounts.json")
DEVICE_MAP_FILE = os.path.join(APP_DATA, "device_map.json")

EP_CHAT = "/api/agent/v3/llm_utils_chat"
APP_ID = "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8"
ASSISTANT_HOST = "https://api5-normal.mchost.guru"
ASSISTANT_IDE_VERSION = "0.1.50"
ASSISTANT_IDE_VERSION_CODE = "20260811"

CAPTURED_HELIOS = "ZXUAAJY4uyadZMga00bKJuXKYe+N3WVyULzf6GWYGwgUpM5v"
CAPTURED_MEDUSA = (
    "ee1+ao0FIZCtOqJRrMe2Gi0iH3KbKwMBQY1zRmDAEaw0mci6VSJnXjJwQoH7Ooel"
    "YZSSmBTE4Xh4cg560l/7uvZGqDu9R9yBwiXfKhOyWGFOKMHGyvz6JS8lTLHLnWAd"
    "brVcub28qpRKMEK1X99PyccVcBrg3Ttd1KaqDRiUFXicYKxXvmH2yn0knR8eWu3H"
    "krneQB0ouMnHleTYvN4sWd2axJjCfLceqIQWWrL50/v22PmNtzzaP6ZHUB7pP8te"
    "3bmpJAUhxP+Ycy2kUtVoYpgksSFaWnkOEtYvsXKQo1MXs9RkA1RtOT/nlEmjHf6RD"
    "bp/LmvtmW68zkRuS8P7g1xs0FBmU9VjnLFlptWa1XPCiRj1H3rP3igUNdktgBQPR"
    "iw+G7/R6C+sLxI8tdYThUr2+mddyeIfzrQjHdjNaROy+po4G53okbu5tTQoGf/Rk"
    "UXAID9nHjAxggmdRJnrYqyMbdfu7t6R/G+fXwBKXwaC+HBQuOpAazPc6qxWp6BkmK"
    "pziQ2B/TjEFMxhFZHoFO5rlQA9eA+v+1gPjrKT5CGL0GSdXj2N6oYbQRB1mcb0pkO"
    "a5sw5btlwlBEkc+YVZOf88wY3hzsM/93PDllXIldThZAHgYESVPIUJJo3EovviQ4A"
    "IKSmF7PvfIGCmGhKQ9+8R5+fj88fHs/f5d3PF4fpJ5yaIOsx7vmyJwKym28RN/+Vu"
    "a+JO41q6lk+WKy9N75NlY1Wbl+0Sw2op8Gnya+3HtS6Tyd1CeSjmGiVkBcoH+X//9"
    "+/7/f//wAA"
)
CAPTURED_NEPTUNE = "-11|50:51:59:00:09"

HARDCODED_DEVICE_ID = "199439841787403"
HARDCODED_MACHINE_ID = "b04f40320d4f2d7173a83374cd9f2df3e635907e386a0cdb4612e4fcb37c97e1"


def seeded_hex(n: int, seed: str, salt: str = "mach") -> str:
    data = f"{salt}:{seed}".encode("utf-8")
    out = b""
    i = 0
    while len(out) < (n + 1) // 2:
        out += hashlib.sha256(data + i.to_bytes(4, "big")).digest()
        i += 1
    return "".join(f"{b:02x}" for b in out[: (n + 1) // 2])[:n]


def load_accounts() -> list:
    with open(ACCOUNTS_FILE, "r", encoding="utf-8") as f:
        data = json.load(f)
    return data.get("accounts", [])


def load_device_map() -> dict:
    with open(DEVICE_MAP_FILE, "r", encoding="utf-8") as f:
        return json.load(f)


def normalize_jwt(jwt: str) -> str:
    if jwt.startswith("Cloud-IDE-JWT "):
        return jwt[len("Cloud-IDE-JWT "):]
    return jwt


def build_headers(account, device_map, use_hardcoded_device=False):
    """构建请求头：完整认证 + 安全头 + 设备ID"""
    jwt_raw = normalize_jwt(account["jwt"])
    uid = account["UserID"]
    dev = device_map.get(uid, {})

    if use_hardcoded_device:
        device_id = HARDCODED_DEVICE_ID
        machine_id = HARDCODED_MACHINE_ID
    else:
        device_id = dev.get("device_id", HARDCODED_DEVICE_ID)
        machine_id = seeded_hex(64, uid, salt="mach")

    headers = {
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
        "User-Agent": f"Trae/{ASSISTANT_IDE_VERSION}",
        "Authorization": f"Cloud-IDE-JWT {jwt_raw}",
        "X-Cloudide-Token": jwt_raw,
        "X-Ide-Token": jwt_raw,
        "X-Uid": uid,
        "X-App-Id": APP_ID,
        "X-App-Version": "default",
        "X-Ide-Version": ASSISTANT_IDE_VERSION,
        "X-Ide-Version-Code": ASSISTANT_IDE_VERSION_CODE,
        "X-App-Version-Code": ASSISTANT_IDE_VERSION_CODE,
        "X-Ide-Version-Type": "stable",
        "X-Device-Type": "windows",
        "X-OS-Version": "Windows 11 Home China",
        "X-Device-Brand": "CREFG-XX",
        "Request-Traffic-Type": "prod",
        "X-Machine-Id": machine_id,
        "X-Device-Id": device_id,
        "X-Lgw-Req-Sdk-Type": "3",
        "Package-Type": "stable_cn",
        "Referer": f"https://trae-api-cn.mchost.guru{EP_CHAT}",
        "X-Helios": CAPTURED_HELIOS,
        "X-Medusa": CAPTURED_MEDUSA,
        "X-Neptune": CAPTURED_NEPTUNE,
    }
    return {
        "url": ASSISTANT_HOST + EP_CHAT,
        "headers": headers,
    }


def send_request(url: str, headers: dict, body: bytes, timeout: int = 30) -> dict:
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    try:
        start = time.time()
        resp = urllib.request.urlopen(req, timeout=timeout, context=ctx)
        elapsed_ms = int((time.time() - start) * 1000)
        raw = resp.read(16384)
        status = resp.status
        text = raw.decode("utf-8", errors="replace")
        return {
            "status": status,
            "elapsed_ms": elapsed_ms,
            "body": text[:2000],
            "error": None,
        }
    except urllib.error.HTTPError as e:
        elapsed_ms = int((time.time() - start) * 1000) if "start" in dir() else 0
        body = ""
        try:
            body = e.read(2048).decode("utf-8", errors="replace")
        except Exception:
            pass
        return {
            "status": e.code,
            "elapsed_ms": elapsed_ms,
            "body": body[:2000],
            "error": f"HTTP {e.code}: {e.reason}",
        }
    except Exception as e:
        return {
            "status": 0,
            "elapsed_ms": 0,
            "body": "",
            "error": str(e),
        }


def extract_credits(body: str) -> dict:
    """从 SSE 响应中提取积分信息"""
    info = {}
    try:
        # 找 notify_usage 事件
        if "cn_credits_remain_info" in body:
            import re
            m = re.search(r'"ide_credits":(\d+\.?\d*)', body)
            if m:
                info["ide_credits"] = float(m.group(1))
            m = re.search(r'"work_credits":(\d+\.?\d*)', body)
            if m:
                info["work_credits"] = float(m.group(1))
        if '"code":4008' in body or "exceeded the quota" in body:
            info["error"] = "quota_exceeded"
        elif '"code":2001' in body:
            info["error"] = "app_config_not_found"
        elif "event:output" in body or '"response"' in body:
            info["error"] = None
            info["success"] = True
        elif '"code":' in body:
            m = re.search(r'"code":(\d+)', body)
            if m:
                info["error"] = f"code_{m.group(1)}"
    except Exception:
        pass
    return info


# 测试矩阵：不同 function + 额外参数组合
TEST_CASES = [
    # (label, function, extra_params)
    ("solo_work_lite (基准)", "solo_work_lite", {}),
    ("solo_agent_lite", "solo_agent_lite", {}),
    ("solo_agent", "solo_agent", {}),
    ("builder", "builder", {}),
    ("builder_v3", "builder_v3", {}),
    ("chat_v3", "chat_v3", {}),
    ("solo_agent_lite +mode_type=1", "solo_agent_lite", {"mode_type": 1}),
    ("solo_agent_lite +trae_request_type=0", "solo_agent_lite", {"trae_request_type": 0}),
    ("solo_agent_lite +trae_request_type=1", "solo_agent_lite", {"trae_request_type": 1}),
    ("solo_agent +mode_type=1", "solo_agent", {"mode_type": 1}),
    ("builder +mode_type=1", "builder", {"mode_type": 1}),
    ("chat_v3 +mode_type=1", "chat_v3", {"mode_type": 1}),
]


def run_test(accounts: list, device_map: dict, account_filter: str = None, use_hardcoded: bool = False):
    print("=" * 80)
    print("深度参数测试 — 不同 function + mode_type 组合")
    print("=" * 80)
    print(f"使用安全头透传: 是")
    print(f"使用硬编码设备ID: {'是' if use_hardcoded else '否(每账号设备ID)'}")
    print(f"测试用例: {len(TEST_CASES)} 种")
    print()

    for account in accounts:
        name = account["name"]
        if account_filter and name != account_filter:
            continue

        uid = account["UserID"]
        dev = device_map.get(uid, {})
        print(f"┌─ 账号: {name} (uid={uid})")
        if dev:
            print(f"│  设备: device_id={dev.get('device_id', 'N/A')}")

        for i, (label, function_name, extra_params) in enumerate(TEST_CASES):
            body = {
                "max_tokens": 100,
                "messages": [{"content": [{"text": "hi", "type": "text"}], "role": "user"}],
                "stream": True,
                "config_name": "glm-5.2",
                "function": function_name,
                "model": "glm-5.2",
            }
            body.update(extra_params)
            body_bytes = json.dumps(body).encode("utf-8")

            config = build_headers(account, device_map, use_hardcoded_device=use_hardcoded)
            print(f"│")
            print(f"├─ [{i+1}/{len(TEST_CASES)}] {label}")

            os.environ["NO_PROXY"] = "*"
            result = send_request(config["url"], config["headers"], body_bytes, timeout=30)

            credits_info = extract_credits(result["body"])
            is_success = credits_info.get("success", False)
            status_icon = "✅" if is_success else "❌"

            print(f"│  结果: {status_icon} HTTP {result['status']} ({result['elapsed_ms']}ms)", end="")

            if result["error"]:
                print(f" | 错误: {result['error']}")
            elif is_success:
                print(f" | ✅ 正常输出!")
            elif "ide_credits" in credits_info or "work_credits" in credits_info:
                ide = credits_info.get("ide_credits", "?")
                work = credits_info.get("work_credits", "?")
                err = credits_info.get("error", "?")
                print(f" | ide={ide}, work={work}, err={err}")
            else:
                preview = result["body"][:300].replace("\n", "\\n")
                print(f" | {preview}")

            if is_success:
                print(f"│  → 找到可用配置！")
                break

            time.sleep(1)

        print(f"└{'─' * 78}")
        print()

    print("=" * 80)
    print("测试完成")
    print("=" * 80)


def main():
    parser = argparse.ArgumentParser(description="深度参数测试")
    parser.add_argument("--account", type=str, help="只测试指定账号")
    parser.add_argument("--hardcoded-device", action="store_true", help="使用硬编码设备ID")
    args = parser.parse_args()

    accounts = load_accounts()
    device_map = load_device_map()
    run_test(accounts, device_map, account_filter=args.account, use_hardcoded=args.hardcoded_device)


if __name__ == "__main__":
    main()
