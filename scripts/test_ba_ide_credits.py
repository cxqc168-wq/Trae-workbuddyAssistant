#!/usr/bin/env python3
"""
测试 ba_* 开头账号的 IDE 积分是否可用
通过 llm_utils_chat 端点发送最小请求，检查响应中的积分状态

用法:
    python test_ba_ide_credits.py              # 测试所有 ba_* 账号
    python test_ba_ide_credits.py --account 0  # 测试第一个 ba_* 账号
"""
import json, ssl, http.client, uuid, hashlib, os, sys, time
from pathlib import Path

# ── 配置 ──────────────────────────────────────────────
HOST = "trae-api-cn.mchost.guru"
ENDPOINT = "/api/agent/v3/llm_utils_chat"
APP_ID = "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8"
IDE_VERSION = "0.1.50"
IDE_VERSION_CODE = "20260811"

APP_DATA = Path(os.environ.get("APPDATA", "")) / "TraeWorkAssistant" / "data"


def load_ba_accounts():
    """加载 ba_* 开头的账号"""
    raw = json.loads((APP_DATA / "checkin_accounts.json").read_text("utf-8"))
    accs = raw.get("accounts", raw) if isinstance(raw, dict) else raw
    return [a for a in accs if a.get("name", "").startswith("ba_")]


def load_device_map():
    """加载 device_map.json"""
    p = APP_DATA / "device_map.json"
    if p.exists():
        return json.loads(p.read_text("utf-8"))
    return {}


def seeded_hex(n, seed, salt="mach"):
    """与 Rust seeded_hex 算法一致，从 uid 生成 machine_id"""
    data = f"{salt}:{seed}"
    out = bytearray()
    i = 0
    while len(out) < (n + 1) // 2:
        h = hashlib.sha256(data.encode() + i.to_bytes(4, "big")).digest()
        out.extend(h)
        i += 1
    hex_str = out[: (n + 1) // 2].hex()
    return hex_str[:n]


def build_headers(jwt, uid, device_id, machine_id):
    """构建 llm_utils_chat 请求头"""
    trace_id = uuid.uuid4().hex
    return {
        "content-type": "application/json",
        "accept": "*/*",
        "accept-encoding": "gzip, deflate, br",
        "user-agent": "TraeClient/TTNet",
        "x-ide-token": jwt,
        "x-app-id": APP_ID,
        "x-app-version": "default",
        "x-app-version-code": IDE_VERSION_CODE,
        "x-ide-version": IDE_VERSION,
        "x-ide-version-code": IDE_VERSION_CODE,
        "x-ide-version-type": "stable",
        "x-device-type": "windows",
        "x-device-brand": "CREFG-XX",
        "x-device-cpu": "Intel",
        "x-device-id": device_id,
        "x-machine-id": machine_id,
        "x-os-version": "Windows 11 Home China",
        "request-traffic-type": "prod",
        "package-type": "stable_cn",
        "x-lgw-req-sdk-type": "3",
        "x-lscbd-aid": "787976",
        "x-lscbd-platform": "windows",
        "x-ss-dp": "787976",
        "app-version": IDE_VERSION,
        "x-custom-trace-id": trace_id[:16],
        "x-flow-traceparent": f"04-{trace_id}-{uuid.uuid4().hex[:16]}-01",
        "x-tt-trace-id": f"00-{trace_id}-{uuid.uuid4().hex[:16]}-01",
        "x-request-id": f"req_{trace_id}",
        "referer": f"https://{HOST}{ENDPOINT}",
    }


def build_body(uid, device_id, machine_id):
    """构建最小 llm_utils_chat 请求体"""
    return json.dumps({
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "model": "DeepSeek-V4-Flash",
        "config_name": "DeepSeek-V4-Flash",
        "model_name": "deepseek_v4_flash__dev",
        "stream": True,
        "function": "solo_work_lite",
        "max_tokens": 16,
        "conversation_id": str(uuid.uuid4()),
        "user_id": uid,
        "session_id": str(uuid.uuid4()),
        "device_id": device_id,
        "machine_id": machine_id,
        "project_id": str(uuid.uuid4()),
        "workspace_id": "e04cdd",
        "prompt_max_tokens": 168000,
        "mode": "FunctionCall",
        "ide_version": IDE_VERSION,
        "ide_version_code": IDE_VERSION_CODE,
        "app_id": APP_ID,
        "package_type": "stable_cn",
    }, ensure_ascii=False).encode("utf-8")


def send_request(account, device_map):
    """发送 llm_utils_chat 请求，返回 (status, events)"""
    name = account["name"]
    uid = str(account["UserID"])
    jwt = account.get("jwt", "").replace("Cloud-IDE-JWT ", "").strip()

    # device_id: 优先用账号自带，其次用 device_map
    dm = device_map.get(uid, {})
    device_id = account.get("device_id") or dm.get("device_id", "")
    machine_id = dm.get("machine_id") or seeded_hex(64, uid)

    headers = build_headers(jwt, uid, device_id, machine_id)
    body = build_body(uid, device_id, machine_id)

    ctx = ssl.create_default_context()
    conn = http.client.HTTPSConnection(HOST, context=ctx, timeout=60)
    try:
        conn.request("POST", ENDPOINT, body=body, headers=headers)
        resp = conn.getresponse()
        # 读取前 4KB 足够判断状态
        data = resp.read(4096)
        return resp.status, data
    finally:
        conn.close()


def parse_sse_events(data):
    """从响应数据中提取 SSE 事件"""
    text = data.decode("utf-8", "replace")
    events = []
    current_event = ""
    current_data = ""

    for line in text.split("\n"):
        line = line.strip("\r")
        if line.startswith("event:"):
            current_event = line[6:].strip()
        elif line.startswith("data:"):
            current_data += line[5:]
        elif line == "" and current_event:
            events.append((current_event, current_data))
            current_event = ""
            current_data = ""

    if current_event and current_data:
        events.append((current_event, current_data))

    return events


def main():
    import argparse
    parser = argparse.ArgumentParser(description="测试 ba_* 账号 IDE 积分")
    parser.add_argument("--account", type=int, default=-1, help="指定测试第几个 ba_* 账号（从0开始），不指定则全部测试")
    args = parser.parse_args()

    accounts = load_ba_accounts()
    if not accounts:
        print("未找到 ba_* 开头的账号")
        sys.exit(1)

    device_map = load_device_map()

    if args.account >= 0:
        if args.account >= len(accounts):
            print(f"账号索引超出范围（共 {len(accounts)} 个 ba_* 账号）")
            sys.exit(1)
        accounts = [accounts[args.account]]

    print(f"找到 {len(accounts)} 个 ba_* 账号，开始测试...\n")
    print(f"上游: {HOST}{ENDPOINT}")
    print(f"积分: IDE 积分 (product_id 208)\n")
    print("=" * 70)

    for i, acc in enumerate(accounts):
        name = acc["name"]
        uid = str(acc["UserID"])
        print(f"\n[{i}] {name} (uid={uid})")

        try:
            status, data = send_request(acc, device_map)
            text = data.decode("utf-8", "replace")

            if status == 200:
                events = parse_sse_events(data)
                has_output = False
                has_error = False
                error_msg = ""
                usage_info = ""

                for ev_name, ev_data in events:
                    if ev_name == "output":
                        has_output = True
                    elif ev_name == "error":
                        has_error = True
                        try:
                            err = json.loads(ev_data)
                            error_msg = f"code={err.get('code','?')} msg={err.get('message','?')}"
                        except:
                            error_msg = ev_data[:100]
                    elif ev_name == "notify_usage":
                        try:
                            usage_info = json.loads(ev_data)
                        except:
                            pass
                    elif ev_name == "token_usage":
                        try:
                            usage_info = json.loads(ev_data)
                        except:
                            pass

                if has_error:
                    print(f"  结果: ERROR — {error_msg}")
                elif has_output:
                    print(f"  结果: SUCCESS — 收到模型回复")
                    if usage_info:
                        print(f"  积分: {json.dumps(usage_info, ensure_ascii=False)[:200]}")
                else:
                    # 可能是 notify_usage 但无 output（积分不足）
                    first_data = text[:300]
                    if "notify_usage" in text or "cn_credits" in text:
                        print(f"  结果: 积分不足（收到 notify_usage 但无 output）")
                        # 尝试提取积分信息
                        for line in text.split("\n"):
                            if "data:" in line and ("credits" in line or "notify" in line):
                                print(f"  {line.strip()[:200]}")
                    else:
                        print(f"  结果: 未知响应（前300字符）")
                        print(f"  {first_data}")
            else:
                print(f"  结果: HTTP {status}")
                print(f"  {text[:300]}")

        except Exception as e:
            print(f"  结果: 异常 — {e}")

        time.sleep(1)  # 避免请求过快

    print("\n" + "=" * 70)
    print("测试完成")


if __name__ == "__main__":
    main()
