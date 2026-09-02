#!/usr/bin/env python3
"""
上游 API 请求测试脚本 — 对比 trae-work-assistant 与 traework2api 两套请求头配置。
测试目标：定位 "quota exceeded" / "no healthy account" 的根因。

用法:
  python scripts/test_upstream.py                         # 测试所有配置 × 所有账号
  python scripts/test_upstream.py --account liu_gs        # 只测试指定账号
  python scripts/test_upstream.py --config B              # 只测试指定配置
  python scripts/test_upstream.py --matrix                # 模型名×function 矩阵测试
"""

import json
import os
import sys
import time
import hashlib
import argparse
import urllib.request
import urllib.error
import ssl

# ─── 路径常量 ───
APP_DATA = os.path.join(os.environ.get("APPDATA", ""), "TraeWorkAssistant", "data")
ACCOUNTS_FILE = os.path.join(APP_DATA, "checkin_accounts.json")
DEVICE_MAP_FILE = os.path.join(APP_DATA, "device_map.json")

# ─── 上游常量 ───
EP_CHAT = "/api/agent/v3/llm_utils_chat"
APP_ID = "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8"

# trae-work-assistant 版本（当前项目）
ASSISTANT_HOST = "https://trae-api-cn.mchost.guru"
ASSISTANT_IDE_VERSION = "0.1.50"
ASSISTANT_IDE_VERSION_CODE = "20260811"

# traework2api 版本（参考项目）
TW2A_HOST = "https://trae-api-cn.mchost.guru"
TW2A_IDE_VERSION = "0.1.43"
TW2A_IDE_VERSION_CODE = "20260716"

# 代理抓包的安全头（来自 ba_1855 账号的 Trae 客户端请求）
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

# 硬编码的设备指纹（当前 trae-work-assistant 使用的值）
HARDCODED_DEVICE_ID = "199439841787403"
HARDCODED_MACHINE_ID = "b04f40320d4f2d7173a83374cd9f2df3e635907e386a0cdb4612e4fcb37c97e1"

CHAT_BODY_TEMPLATE = {
    "max_tokens": 100,
    "messages": [{"content": [{"text": "hi", "type": "text"}], "role": "user"}],
    "stream": True,
}

# 测试矩阵：不同 config_name + function 组合
TEST_MATRIX = [
    ("glm-5.2", "solo_work_lite"),
    ("glm-5.2", "solo_agent_lite"),
    ("Doubao-Seed-2.1-Pro", "solo_work_lite"),
    ("Doubao-Seed-2.1-Pro", "solo_agent_lite"),
    ("Doubao-Seed-2.1-Turbo", "solo_work_lite"),
    ("Doubao-Seed-2.1-Turbo", "solo_agent_lite"),
]


def seeded_hex(n: int, seed: str, salt: str = "mach") -> str:
    """确定性派生 hex 字符串（与 device_proxy.py 算法一致）"""
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
    """返回不带 'Cloud-IDE-JWT ' 前缀的纯 token"""
    if jwt.startswith("Cloud-IDE-JWT "):
        return jwt[len("Cloud-IDE-JWT "):]
    return jwt


def build_headers_config_a(account, device_map):
    """配置 A：当前 trae-work-assistant 的请求头（硬编码设备ID，仅 x-ide-token）"""
    jwt_raw = normalize_jwt(account["jwt"])
    return {
        "url": ASSISTANT_HOST + EP_CHAT,
        "headers": {
            "content-type": "application/json",
            "accept": "*/*",
            "accept-encoding": "gzip, deflate, br, zstd",
            "user-agent": "TraeClient/TTNet",
            "x-ide-token": jwt_raw,
            "x-app-id": APP_ID,
            "x-app-version": "default",
            "x-app-version-code": ASSISTANT_IDE_VERSION_CODE,
            "x-ide-version": ASSISTANT_IDE_VERSION,
            "x-ide-version-code": ASSISTANT_IDE_VERSION_CODE,
            "x-ide-version-type": "stable",
            "x-device-type": "windows",
            "x-device-brand": "CREFG-XX",
            "x-device-cpu": "Intel",
            "x-device-id": HARDCODED_DEVICE_ID,
            "x-machine-id": HARDCODED_MACHINE_ID,
            "x-os-version": "Windows 11 Home China",
            "request-traffic-type": "prod",
            "package-type": "stable_cn",
            "x-bridge-transport": "aha",
            "x-lgw-req-sdk-type": "3",
            "x-lscbd-aid": "787976",
            "x-lscbd-platform": "windows",
            "x-ss-dp": "787976",
            "app-version": ASSISTANT_IDE_VERSION,
            "referer": f"https://trae-api-cn.mchost.guru{EP_CHAT}",
        },
    }


def build_headers_config_b(account, device_map):
    """配置 B：traework2api 风格（trae-api-cn host + 完整认证头 + 每账号设备ID）"""
    jwt_raw = normalize_jwt(account["jwt"])
    uid = account["UserID"]
    dev = device_map.get(uid, {})
    device_id = dev.get("device_id", "")
    machine_id = seeded_hex(64, uid, salt="mach")

    return {
        "url": TW2A_HOST + EP_CHAT,
        "headers": {
            "Content-Type": "application/json",
            "Accept": "text/event-stream",
            "User-Agent": f"Trae/{TW2A_IDE_VERSION}",
            "Authorization": f"Cloud-IDE-JWT {jwt_raw}",
            "X-Cloudide-Token": jwt_raw,
            "X-Ide-Token": jwt_raw,
            "X-Uid": uid,
            "X-App-Id": APP_ID,
            "X-App-Version": "default",
            "X-Ide-Version": TW2A_IDE_VERSION,
            "X-Ide-Version-Code": TW2A_IDE_VERSION_CODE,
            "X-App-Version-Code": TW2A_IDE_VERSION_CODE,
            "X-Ide-Version-Type": "stable",
            "X-Device-Type": "windows",
            "X-OS-Version": "Windows 11 Pro",
            "X-Device-Brand": "83DG",
            "Request-Traffic-Type": "prod",
            "X-Machine-Id": machine_id,
            "X-Device-Id": device_id,
        },
    }


def build_headers_config_c(account, device_map):
    """配置 C：混合模式（api5-normal host + 完整认证头 + 每账号设备ID + 当前版本号）"""
    jwt_raw = normalize_jwt(account["jwt"])
    uid = account["UserID"]
    dev = device_map.get(uid, {})
    device_id = dev.get("device_id", "")
    machine_id = seeded_hex(64, uid, salt="mach")

    return {
        "url": ASSISTANT_HOST + EP_CHAT,
        "headers": {
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
        },
    }


def build_headers_config_d(account, device_map):
    """配置 D：完整模式（配置 C + 透传 x-helios/x-medusa/x-neptune）"""
    config = build_headers_config_c(account, device_map)
    config["headers"]["X-Helios"] = CAPTURED_HELIOS
    config["headers"]["X-Medusa"] = CAPTURED_MEDUSA
    config["headers"]["X-Neptune"] = CAPTURED_NEPTUNE
    return config


CONFIGS = {
    "A": ("assistant当前配置(硬编码设备)", build_headers_config_a),
    "B": ("tw2a风格(trae-api-cn+完整认证)", build_headers_config_b),
    "C": ("混合(api5+完整认证+每账号设备)", build_headers_config_c),
    "D": ("完整(C+安全头透传)", build_headers_config_d),
}


def send_request(url: str, headers: dict, body: bytes, timeout: int = 30) -> dict:
    """发送上游请求，返回结果摘要"""
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE

    req = urllib.request.Request(url, data=body, headers=headers, method="POST")

    try:
        start = time.time()
        resp = urllib.request.urlopen(req, timeout=timeout, context=ctx)
        elapsed_ms = int((time.time() - start) * 1000)

        # 读取前 16KB 捕获完整 SSE 事件序列
        raw = resp.read(16384)
        status = resp.status

        # 尝试解码
        text = raw.decode("utf-8", errors="replace")

        # 检测 SSE error 事件
        has_quota_error = "exceeded the quota" in text.lower() or "quota" in text.lower()
        has_error_event = "event:error" in text or '"error"' in text
        has_output = "event:output" in text or '"response"' in text
        has_done = "event:done" in text or "[DONE]" in text

        return {
            "status": status,
            "elapsed_ms": elapsed_ms,
            "body_preview": text[:1600],
            "has_quota_error": has_quota_error,
            "has_error_event": has_error_event,
            "has_output": has_output,
            "has_done": has_done,
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
            "body_preview": body[:800],
            "has_quota_error": "quota" in body.lower(),
            "has_error_event": True,
            "has_output": False,
            "has_done": False,
            "error": f"HTTP {e.code}: {e.reason}",
        }
    except Exception as e:
        return {
            "status": 0,
            "elapsed_ms": 0,
            "body_preview": "",
            "has_quota_error": False,
            "has_error_event": False,
            "has_output": False,
            "has_done": False,
            "error": str(e),
        }


def run_config_test(accounts: list, device_map: dict, config_filter: str = None, account_filter: str = None):
    """对所有配置 × 账号测试 glm-5.2 + solo_work_lite"""
    print("=" * 80)
    print("上游 API 请求测试 — 4种配置 × 账号 (glm-5.2 + solo_work_lite)")
    print("=" * 80)
    print(f"账号文件: {ACCOUNTS_FILE}")
    print(f"设备映射: {DEVICE_MAP_FILE}")
    print(f"账号数量: {len(accounts)}")
    print()

    body = dict(CHAT_BODY_TEMPLATE)
    body["config_name"] = "glm-5.2"
    body["function"] = "solo_work_lite"
    body["model"] = "glm-5.2"
    body_bytes = json.dumps(body).encode("utf-8")

    for account in accounts:
        name = account["name"]
        if account_filter and name != account_filter:
            continue

        uid = account["UserID"]
        dev = device_map.get(uid, {})
        print(f"┌─ 账号: {name} (uid={uid})")
        if dev:
            print(f"│  设备: device_id={dev.get('device_id', 'N/A')}")
        else:
            print(f"│  设备: ❌ 未在 device_map 中找到")

        for cfg_key, (cfg_label, cfg_builder) in CONFIGS.items():
            if config_filter and cfg_key != config_filter:
                continue

            config = cfg_builder(account, device_map)
            print(f"│")
            print(f"├─ 配置 {cfg_key}: {cfg_label}")

            os.environ["NO_PROXY"] = "*"
            result = send_request(config["url"], config["headers"], body_bytes, timeout=30)

            status_icon = "✅" if result["has_output"] else "❌"
            if result["error"] and not result["status"]:
                status_icon = "💥"

            print(f"│  结果: {status_icon} HTTP {result['status']} ({result['elapsed_ms']}ms)")

            if result["error"]:
                print(f"│  错误: {result['error']}")

            # 始终打印 body 预览
            preview = result["body_preview"][:800].replace("\n", "\\n")
            if preview:
                print(f"│  Body: {preview}")

            if result["has_output"]:
                print(f"│  → ✅ 正常输出!")

            time.sleep(1)

        print(f"└{'─' * 78}")
        print()

    print("=" * 80)
    print("测试完成")
    print("=" * 80)


def run_matrix_test(accounts: list, device_map: dict, config_filter: str = None, account_filter: str = None):
    """模型名 × function 名矩阵测试（使用指定配置）"""
    cfg_key = config_filter or "B"
    cfg_label, cfg_builder = CONFIGS.get(cfg_key, CONFIGS["B"])

    print("=" * 80)
    print(f"上游 API 请求测试 — 模型名 × function 名矩阵 (配置 {cfg_key}: {cfg_label})")
    print("=" * 80)
    print(f"测试矩阵: {len(TEST_MATRIX)} 种 config_name × function 组合")
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

        found_working = False
        for config_name, function_name in TEST_MATRIX:
            body = dict(CHAT_BODY_TEMPLATE)
            body["config_name"] = config_name
            body["function"] = function_name
            body["model"] = config_name
            body_bytes = json.dumps(body).encode("utf-8")

            config = cfg_builder(account, device_map)
            label = f"config_name={config_name}, function={function_name}"
            print(f"│")
            print(f"├─ {label}")

            os.environ["NO_PROXY"] = "*"
            result = send_request(config["url"], config["headers"], body_bytes, timeout=30)

            status_icon = "✅" if result["has_output"] else "❌"
            if result["error"] and not result["status"]:
                status_icon = "💥"

            print(f"│  结果: {status_icon} HTTP {result['status']} ({result['elapsed_ms']}ms)")

            if result["error"]:
                print(f"│  错误: {result['error']}")

            # 始终打印 body 预览
            preview = result["body_preview"][:400].replace("\n", "\\n")
            if preview:
                print(f"│  Body: {preview}")

            if result["has_output"]:
                print(f"│  → ✅ 正常输出!")
                found_working = True

            if found_working:
                break

            time.sleep(2)

        print(f"└{'─' * 78}")
        print()

    print("=" * 80)
    print("测试完成")
    print("=" * 80)


def main():
    parser = argparse.ArgumentParser(description="上游 API 请求测试")
    parser.add_argument("--account", type=str, help="只测试指定账号 (按 name 过滤)")
    parser.add_argument("--config", type=str, choices=["A", "B", "C", "D"], help="只测试指定配置")
    parser.add_argument("--matrix", action="store_true", help="运行模型名×function 矩阵测试")
    args = parser.parse_args()

    accounts = load_accounts()
    device_map = load_device_map()

    if args.matrix:
        run_matrix_test(accounts, device_map, config_filter=args.config, account_filter=args.account)
    else:
        run_config_test(accounts, device_map, config_filter=args.config, account_filter=args.account)


if __name__ == "__main__":
    main()
