#!/usr/bin/env python3
# -*- coding: utf-8 -*-
r"""
Trae Work 多账号自动签到脚本 — Trae Work 助手 内置版
=====================================================
读取 checkin_accounts.json 中保存的各账号 JWT，按账号分配稳定伪设备 ID
（与 device_proxy.py 共用 device_map.json），批量调用签到接口领取积分。

JWT 获取方式：
1. 启动 device_proxy.py 并启动 TRAE 走代理；
2. 在 TRAE 里切换每个账号并点击签到；
3. 代理日志 (proxy.log) 会打印出 [JWT 自动更新] user=... 及对应的 Authorization 头。

环境变量 TRAEDATA_DIR 可重定向数据文件位置（桌面端用它指向 %APPDATA%\TraeWorkAssistant）。

命令行参数：
    --json-stream   以单行 JSON（NDJSON）输出每账号结果，供桌面端逐条渲染
    --accounts U1,U2   仅签指定 UserID（逗号分隔）
    --scope all|group:<id>   兼容参数（实际过滤由桌面端通过 --accounts 下发）

把 JWT 按如下格式填入 checkin_accounts.json：
{
  "accounts": [
    {"name": "me_1676", "UserID": "1234567890123456", "jwt": "Cloud-IDE-JWT eyJ..."}
  ]
}
"""
import os
import sys
import json
import base64
import random
import hashlib
import uuid
import datetime
import urllib.request
import urllib.error
import socket
import gzip
import zlib
import argparse
import time
from typing import Optional

# ─────────────────────────────────────────────────────────────────────────────
# 关键修复：本脚本自带 JWT 与设备标识注入，应「直连」api.trae.cn，
# 绝不能走本地 MITM 代理（127.0.0.1:8899）。
#
# 背景：开启代理时，Rust 侧会把 Windows 系统代理指向 127.0.0.1:8899。
# urllib 在 Windows 上会读取该注册表项，于是签到的请求被路由进本地 MITM 代理；
# 一旦该 Python 代理进程退出（崩溃/被关），端口即变成「死端口」，
# 签到便报 WinError 10061 无法连接。api_server(Rust/ureq) 早已用 NO_PROXY=*
# 规避同一问题，这里与之一致：强制忽略一切代理，直连上游。
# ─────────────────────────────────────────────────────────────────────────────
os.environ["NO_PROXY"] = "*"
os.environ["no_proxy"] = "*"
for _pv in ("HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy"):
    os.environ.pop(_pv, None)
_NO_PROXY_OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))

BASE = os.path.dirname(os.path.abspath(__file__))
# 数据目录：优先 TRAEDATA_DIR（由桌面端注入），否则回退到脚本目录（保持独立可用性）
DATA_DIR = os.environ.get("TRAEDATA_DIR", BASE)
# 子目录结构：conf/ (配置), data/ (数据), logs/ (日志)
CONF_DIR = os.path.join(DATA_DIR, "conf")
DATA_SUBDIR = os.path.join(DATA_DIR, "data")
LOGS_DIR = os.path.join(DATA_DIR, "logs")
for _d in (CONF_DIR, DATA_SUBDIR, LOGS_DIR):
    os.makedirs(_d, exist_ok=True)
ACCOUNTS_FILE = os.path.join(DATA_SUBDIR, "checkin_accounts.json")
MAP_FILE = os.path.join(DATA_SUBDIR, "device_map.json")
SIGNIN_URL = "https://api.trae.cn/trae/api/v2/ug/checkin_credits/claim"
STATUS_URL = "https://api.trae.cn/trae/api/v2/ug/checkin_credits/status"
EXPIRY_WARN_HOURS = 24  # JWT 剩余有效期低于该值时发出告警
LOG_FILE = os.path.join(LOGS_DIR, "checkin.log")

# 伪随机但稳定的生成器，保证同一 user_id 在 device_map.json 缺失时也能复现相同 ID
# 注意：random.Random 是有状态的，必须「每次调用新建」才能保证同 seed 恒等输出，
# 不能缓存对象（缓存会因状态前进导致同 seed 二次调用产出不同序列）。


def _normalize_seed(seed):
    """将任意 seed 归一为稳定 int（字符串走 sha256，跨进程一致；纯数字串按数值）。"""
    if isinstance(seed, int):
        return seed & 0x7FFFFFFFFFFFFFFF
    if isinstance(seed, str):
        if seed.isdigit():
            return int(seed) & 0x7FFFFFFFFFFFFFFF
        return int(hashlib.sha256(seed.encode("utf-8")).hexdigest(), 16) & 0x7FFFFFFFFFFFFFFF
    return seed


def _stable_rng(seed):
    """遗留兼容：基于 seed 的稳定随机数生成器（旧算法，已被 SHA-256 派生取代）。"""
    return random.Random(_normalize_seed(seed))


def _seeded_stream(seed, salt, nbytes):
    """确定性派生均匀字节流（SHA-256），避免 random.Random(seed) 病态序列。"""
    if seed is None:
        return None
    data = f"{salt}:{seed}".encode("utf-8")
    out = b""
    i = 0
    while len(out) < nbytes:
        out += hashlib.sha256(data + i.to_bytes(4, "big")).digest()
        i += 1
    return out[:nbytes]


def rand_digits(n, seed=None):
    if seed is None:
        return "".join(random.choice("0123456789") for _ in range(n))
    bs = _seeded_stream(seed, "devid", n + 1)
    return "".join(str(b % 10) for b in bs[:n])


def rand_hex(n, seed=None):
    if seed is None:
        return "".join(random.choice("0123456789abcdef") for _ in range(n))
    need = (n + 1) // 2
    bs = _seeded_stream(seed, "sess", need)
    return "".join(f"{b:02x}" for b in bs)[:n]


def gen_market_uuid(seed):
    """标准 UUID v4（确定性派生），符合 market_user_id 字段格式。"""
    if seed is None:
        return str(uuid.uuid4())
    bs = bytearray(_seeded_stream(seed, "market", 16))
    bs[6] = (bs[6] & 0x0F) | 0x40
    bs[8] = (bs[8] & 0x3F) | 0x80
    return str(uuid.UUID(bytes=bytes(bs)))


def load_json(path, default=None):
    if default is None:
        default = {}
    if not os.path.exists(path):
        return default
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception as e:
        print(f"[警告] 读取 {path} 失败: {e}")
        return default


def save_json(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    try:
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
        os.replace(tmp, path)
    except Exception as e:
        print(f"[警告] 写入 {path} 失败: {e}")


def save_credits_history(user_id, credits, delta):
    """把账号最新积分与本次新增写入 credits_history.json（按日期追加，自动裁剪到 90 天内）。
    前端积分看板/趋势图消费此文件；此前该文件只被读取而从未写入，导致积分展示恒为 0。"""
    path = os.path.join(DATA_SUBDIR, "credits_history.json")
    data = load_json(path, default={"records": []})
    recs = data.get("records", [])
    today = datetime.datetime.now().strftime("%Y-%m-%d")
    recs.append({"date": today, "user_id": user_id, "credits": credits, "delta": delta})
    cutoff = (datetime.datetime.now() - datetime.timedelta(days=90)).strftime("%Y-%m-%d")
    recs = [r for r in recs if r.get("date", "") >= cutoff]
    data["records"] = recs
    save_json(path, data)


def extract_user_id(jwt):
    """从 JWT payload 里取 data.id，不校验签名。"""
    token = jwt
    if token.startswith("Cloud-IDE-JWT "):
        token = token.split(None, 1)[1]
    parts = token.split(".")
    if len(parts) < 2:
        return None
    try:
        pad = parts[1] + "=" * (-len(parts[1]) % 4)
        payload = json.loads(base64.urlsafe_b64decode(pad))
        data = payload.get("data", {})
        if isinstance(data, dict) and data.get("id"):
            return data.get("id")
        if payload.get("auth_id"):
            return payload.get("auth_id")
        if payload.get("sub"):
            return payload.get("sub")
        return None
    except Exception:
        return None


def get_jwt_exp(jwt):
    """从 JWT payload 取 exp 字段，返回 (exp_datetime, remaining_hours) 或 (None, None)。"""
    token = jwt
    if token.startswith("Cloud-IDE-JWT "):
        token = token.split(None, 1)[1]
    parts = token.split(".")
    if len(parts) < 2:
        return None, None
    try:
        pad = parts[1] + "=" * (-len(parts[1]) % 4)
        payload = json.loads(base64.urlsafe_b64decode(pad))
        exp = payload.get("exp")
        if not isinstance(exp, (int, float)):
            return None, None
        exp_dt = datetime.datetime.fromtimestamp(exp)
        remaining = (exp_dt - datetime.datetime.now()).total_seconds() / 3600.0
        return exp_dt, remaining
    except Exception:
        return None, None


def _decode_body(raw, ce):
    """按 Content-Encoding 解压响应体（标准库覆盖 gzip/deflate）。"""
    ce = (ce or "").lower().strip()
    try:
        if ce == "gzip":
            return gzip.decompress(raw).decode("utf-8", "replace")
        if ce == "deflate":
            return zlib.decompress(raw, -zlib.MAX_WBITS).decode("utf-8", "replace")
    except Exception:
        pass
    return raw.decode("utf-8", "replace")


DEVICE_GEN = 2  # 设备标识生成算法版本；旧记录(gen 缺失=1)会自动重建


def get_device_for(user_id, device_map):
    """
    复用/生成 device_map.json 中该 user_id 的设备标识。
    结构与 device_proxy.py 完全一致，确保代理和脚本看到的映射相同。
    """
    rec = device_map.get(user_id)
    if rec is None or rec.get("gen", 1) < DEVICE_GEN:
        device_map[user_id] = {
            "device_id": rand_digits(15, seed=user_id),
            "market_user_id": gen_market_uuid(user_id),
            "session_id": rand_hex(64, seed=user_id),
            "created": datetime.datetime.now().isoformat(timespec="seconds"),
            "gen": DEVICE_GEN,
        }
        save_json(MAP_FILE, device_map)
    return device_map[user_id]


def _build_headers(jwt, dev):
    """签到/状态接口共用的请求头（按账号独立设备 id 与 session）。"""
    return {
        "accept": "*/*",
        "accept-encoding": "gzip, deflate",
        "accept-language": "zh-CN",
        "authorization": jwt if jwt.startswith("Cloud-IDE-JWT ") else f"Cloud-IDE-JWT {jwt}",
        "content-type": "application/json",
        "user-agent": "VSCode 1.107.1 (TRAE SOLO CN)",
        "x-market-client-id": "VSCode 1.107.1",
        "x-market-user-id": dev["market_user_id"],
        "x-user-region": "CN",
        "x-device-id": dev["device_id"],
        "x-lgw-req-sdk-type": "3",
        "package-type": "stable_cn",
        "x-request-id": str(uuid.uuid4()),
        "x-lscbd-aid": "787976",
        "x-lscbd-platform": "windows",
        "app-version": "0.1.45",
        "x-tt-trace-id": f"00-{uuid.uuid4().hex[:16]}-01",
        "vscode-sessionid": dev["session_id"],
        "sec-fetch-dest": "empty",
        "sec-fetch-mode": "no-cors",
        "sec-fetch-site": "none",
    }


def _http_post(url, jwt, dev, body=b"{}", timeout=30):
    """统一 POST 入口，返回 (status_code:int, body_text:str)。"""
    headers = _build_headers(jwt, dev)
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    try:
        with _NO_PROXY_OPENER.open(req, timeout=timeout) as resp:
            return resp.status, _decode_body(resp.read(), resp.headers.get("Content-Encoding", ""))
    except urllib.error.HTTPError as e:
        return e.code, _decode_body(e.read(), e.headers.get("Content-Encoding", ""))
    except socket.timeout:
        return 0, ""
    except Exception as e:
        return -1, f"{type(e).__name__}: {e}"


def status_check(name, jwt, device_map, timeout=30):
    """预检：返回 (ok: bool, checked_in: bool|None, credits: int|None, code: int|None, message: str)。"""
    user_id = extract_user_id(jwt)
    if not user_id:
        return False, None, None, None, "无法从 JWT 解析 user id"
    dev = get_device_for(user_id, device_map)
    status, body = _http_post(STATUS_URL, jwt, dev, body=b"{}", timeout=timeout)
    if status < 0:
        return False, None, None, None, body or "网络异常"
    try:
        data = json.loads(body)
    except Exception:
        return False, None, None, status, f"非 JSON 响应: {body[:200]}"
    code = data.get("code")
    checked_in = data.get("checked_in")
    credits = data.get("credits")
    msg = data.get("message", "")
    if code != 0:
        return False, checked_in, credits, code, msg or f"HTTP {status}"
    return True, bool(checked_in), credits, code, msg


def signin(name, jwt, device_map, timeout=30):
    """对单个账号执行签到，返回 (success, message, code, http_status)。"""
    user_id = extract_user_id(jwt)
    if not user_id:
        return False, "无法从 JWT 解析 user id", None, 0

    dev = get_device_for(user_id, device_map)
    status, body = _http_post(SIGNIN_URL, jwt, dev, body=b"{}", timeout=timeout)

    if status < 0:
        return False, body or "网络异常", None, status
    try:
        data = json.loads(body)
        return data.get("code") == 0, data.get("message", f"HTTP {status}"), data.get("code"), status
    except Exception:
        return False, f"HTTP {status}: 非 JSON 响应: {body[:200]}", status if status else None, status


def classify_error(http_status, message, code):
    """根据 HTTP 状态码和业务码分类签到错误，返回 (error_type, cooldown_seconds)。
    cooldown_seconds: -1=永久, 0=不冷却(仅记录错误计数), >0=冷却秒数"""
    if http_status == 200 and code == 1005:
        return "PlanLimit", 43200
    if http_status == 429:
        return "SoftRate", 60
    if http_status == 401:
        return "SessionDead", -1
    if http_status == 404:
        return "NotFound", 60
    if 500 <= http_status < 600:
        return "Server", 600
    if 400 <= http_status < 500:
        return "Client", 600
    if code is not None and code != 0:
        return "BusinessError", 300
    return "Unknown", 0


def save_cooldown(user_id, error_type, cooldown_seconds, reason):
    """写入/更新账号冷却状态到 account_cooldowns.json"""
    cooldown_file = os.path.join(DATA_SUBDIR, "account_cooldowns.json")
    data = {}
    try:
        with open(cooldown_file, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        pass
    if not isinstance(data, dict):
        data = {}
    if "cooldowns" not in data or not isinstance(data["cooldowns"], dict):
        data["cooldowns"] = {}

    now = int(time.time())

    if cooldown_seconds == -1:
        until = 9999999999
        error_count = 0
    elif cooldown_seconds == 0:
        data["cooldowns"].pop(user_id, None)
        data["updated_at"] = datetime.datetime.now().isoformat(timespec="seconds")
        save_json(cooldown_file, data)
        return
    else:
        existing = data["cooldowns"].get(user_id, {})
        if error_type in ("Server", "Client"):
            error_count = existing.get("error_count", 0) + 1
            if error_count < 3:
                data["cooldowns"][user_id] = {
                    "type": error_type, "until": 0,
                    "reason": reason, "error_count": error_count,
                }
                data["updated_at"] = datetime.datetime.now().isoformat(timespec="seconds")
                save_json(cooldown_file, data)
                return
            until = now + cooldown_seconds
            error_count = 0
        else:
            until = now + cooldown_seconds
            error_count = 0

    data["cooldowns"][user_id] = {
        "type": error_type, "until": until,
        "reason": reason, "error_count": error_count,
    }
    data["updated_at"] = datetime.datetime.now().isoformat(timespec="seconds")
    save_json(cooldown_file, data)


def clear_cooldown(user_id):
    """清除账号冷却状态"""
    save_cooldown(user_id, "", 0, "")


def signin_with_retry(name, jwt, device_map, timeout=30, retry=0):
    """对单个账号执行签到；仅网络层异常（code 为 None）按 retry 次数重试，业务失败不重试。"""
    last = (False, "无重试", None, 0)
    for attempt in range(retry + 1):
        ok, msg, code, status = signin(name, jwt, device_map, timeout)
        if ok or code is not None:
            return ok, msg, code, status
        last = (ok, msg, code, status)
        if attempt < retry:
            print(f"  [重试] 第 {attempt + 1} 次签到网络异常，1s 后重试…")
            time.sleep(1)
    return last


# ----------------- NDJSON 输出（--json-stream） -----------------
_JSON_STREAM = False


def emit(obj):
    if _JSON_STREAM:
        print(json.dumps(obj, ensure_ascii=False), flush=True)


def main():
    global _JSON_STREAM
    parser = argparse.ArgumentParser(description="Trae Work 多账号自动签到")
    parser.add_argument("--json-stream", action="store_true", help="以 NDJSON 输出每账号结果")
    parser.add_argument("--accounts", default="", help="仅签指定 UserID（逗号分隔）")
    parser.add_argument("--scope", default="all", help="兼容参数（all|group:<id>）")
    parser.add_argument("--retry", type=int, default=0, help="签到网络失败时的重试次数")
    args = parser.parse_args()
    _JSON_STREAM = args.json_stream
    target_uids = (
        {u.strip() for u in args.accounts.split(",") if u.strip()}
        if args.accounts
        else None
    )

    print("=" * 60)
    print("Trae Work 多账号自动签到")
    print("=" * 60)

    accounts_cfg = load_json(ACCOUNTS_FILE, default={"accounts": []})
    accounts = accounts_cfg.get("accounts", [])

    if not accounts:
        print()
        print("ℹ️  checkin_accounts.json 中暂无账号，无需签到。")
        emit({"type": "start", "total": 0})
        emit({"type": "done", "ok": 0, "already": 0, "failed": 0})
        summary = {
            "time": datetime.datetime.now().isoformat(timespec="seconds"),
            "results": [],
            "total_ok": 0,
            "already": 0,
            "failed": 0,
            "warnings": [],
            "note": "no_accounts_yet",
        }
        save_json(os.path.join(DATA_SUBDIR, "checkin_summary.json"), summary)
        return 0

    # 计算本次要处理的账号（受 --accounts 过滤）
    pending = []
    for acc in accounts:
        jwt = acc.get("jwt", "")
        uid = extract_user_id(jwt) if jwt else acc.get("UserID")
        if target_uids and uid not in target_uids:
            continue
        pending.append(acc)

    emit({"type": "start", "total": len(pending)})

    device_map = load_json(MAP_FILE, default={})
    results = []
    warnings = []
    total_ok = 0
    already = 0
    failed = 0

    for idx, acc in enumerate(pending, 1):
        name = acc.get("name", f"账号{idx}")
        jwt = acc.get("jwt", "")
        print(f"\n[{idx}/{len(pending)}] 账号: {name}")
        if not jwt:
            print(f"  结果: 跳过（未配置 jwt）")
            results.append({"name": name, "ok": False, "message": "未配置 jwt"})
            failed += 1
            emit({"type": "account", "index": idx, "user_id": acc.get("UserID", ""), "name": name, "status": "fail", "message": "未配置 jwt"})
            continue

        user_id = extract_user_id(jwt)
        print(f"  user_id: {user_id}")

        exp_dt, remaining = get_jwt_exp(jwt)
        if remaining is not None:
            if remaining < 0:
                print(f"  [WARN] JWT 已过期（{exp_dt:%Y-%m-%d %H:%M}），需重新抓取！")
                warnings.append(f"{name}: JWT 已过期({exp_dt:%Y-%m-%d %H:%M})，请重新抓取")
            elif remaining < EXPIRY_WARN_HOURS:
                print(f"  [WARN] JWT 将于 {remaining:.1f} 小时后过期（{exp_dt:%Y-%m-%d %H:%M}），请尽快重新抓取")
                warnings.append(f"{name}: JWT 将于 {remaining:.1f}h 后过期({exp_dt:%Y-%m-%d %H:%M})，请重新抓取")

        dev = get_device_for(user_id, device_map)
        print(f"  x-device-id: {dev['device_id']} (from device_map.json)")

        ok_s, checked_in, credits_before, code_s, msg_s = status_check(name, jwt, device_map)
        if ok_s and checked_in:
            print(f"  [OK] 已签到（credits={credits_before}），跳过 claim")
            results.append({
                "name": name, "ok": True, "code": 0,
                "action": "skip_already", "credits": credits_before,
                "message": msg_s or "已签到",
            })
            already += 1
            emit({"type": "account", "index": idx, "user_id": user_id, "name": name, "status": "already", "credits": credits_before})
            save_credits_history(user_id, credits_before, 0)
            continue
        if not ok_s:
            print(f"  [WARN] status 预检失败 (code={code_s}) {msg_s} —— 仍尝试 claim")

        ok, msg, code, http_status = signin_with_retry(name, jwt, device_map, retry=args.retry)
        result = {"name": name, "ok": ok, "code": code, "message": msg, "action": "claim"}
        final_credits: Optional[int] = None
        final_delta = 0
        emit_error_type = None
        emit_cooldown_until = None

        if ok:
            # 签到成功 -> 清除冷却
            clear_cooldown(user_id)
            # credits_before 来自 status 接口，表示签到可获得的积分额度
            # 签到成功后，delta 就是该额度（无需再次请求 status 计算差值）
            if isinstance(credits_before, int):
                final_delta = credits_before
                final_credits = credits_before
                result["credits"] = credits_before
                result["credits_delta"] = final_delta
                print(f"  [OK] 签到成功，积分 +{final_delta}")
                result["action"] = "claim_ok"
            else:
                print(f"  [OK] 签到成功（status 未返回积分额度）")
                result["action"] = "claim_ok"
        else:
            # 签到失败 → 分类错误并写入冷却
            error_type, cooldown_secs = classify_error(http_status, msg, code)
            if error_type and error_type != "Unknown":
                save_cooldown(user_id, error_type, cooldown_secs, msg)
                # 读取冷却状态获取 until 值
                cooldown_data = load_json(os.path.join(DATA_SUBDIR, "account_cooldowns.json"), default={})
                cd_entry = cooldown_data.get("cooldowns", {}).get(user_id, {})
                emit_error_type = error_type
                emit_cooldown_until = cd_entry.get("until", 0)
                print(f"  [COOLDOWN] {error_type} 冷却 {cooldown_secs}s (until={emit_cooldown_until})")

        results.append(result)
        print(f"  结果: {'成功' if ok else '失败'} (code={code}) {msg}")
        # 落盘积分历史（供前端看板/趋势），并回传余额让实时进度不再显示「余额 ?」
        if final_credits is not None:
            save_credits_history(user_id, final_credits, final_delta)
        emit({
            "type": "account",
            "index": idx,
            "user_id": user_id,
            "name": name,
            "status": "success" if ok else "fail",
            "code": code,
            "message": msg,
            "credits": final_credits,
            "delta": final_delta if final_delta else None,
            "error_type": emit_error_type,
            "cooldown_until": emit_cooldown_until,
        })
        if ok:
            total_ok += 1
        else:
            failed += 1

    print("\n" + "=" * 60)
    print(f"签到完成: 成功 {total_ok} / 已签到 {already} / 失败 {failed} / 总计 {len(pending)}")
    print("=" * 60)

    if warnings:
        print("\n[WARN]  JWT 过期告警：")
        for w in warnings:
            print(f"   - {w}")

    summary = {
        "time": datetime.datetime.now().isoformat(timespec="seconds"),
        "results": [{k: v for k, v in r.items() if k != "jwt"} for r in results],
        "total_ok": total_ok,
        "already": already,
        "failed": failed,
        "warnings": warnings,
    }
    save_json(os.path.join(DATA_SUBDIR, "checkin_summary.json"), summary)
    print(f"结果摘要已保存: {os.path.join(DATA_SUBDIR, 'checkin_summary.json')}")

    log_line = (
        f"[{datetime.datetime.now():%Y-%m-%d %H:%M:%S}] "
        f"成功{total_ok}/已签到{already}/失败{failed}/总计{len(pending)}"
        + (f" | 告警: {'; '.join(warnings)}" if warnings else "")
        + "\n"
    )
    try:
        with open(LOG_FILE, "a", encoding="utf-8") as f:
            f.write(log_line)
    except Exception as e:
        print(f"[警告] 写入 {LOG_FILE} 失败: {e}")

    emit({"type": "done", "ok": total_ok, "already": already, "failed": failed})
    return 0


if __name__ == "__main__":
    sys.exit(main() or 0)
