#!/usr/bin/env python3
# -*- coding: utf-8 -*-
r"""
Trae Work 签到设备ID代理 (方案A) — Trae Work 助手 内置版
========================================================
本地 MITM 代理：透明截获 TRAE 全量 HTTPS 流量（对抵达代理的所有 CONNECT 域名做
TLS 解密，覆盖 api.trae.cn / api.trae.com.cn / zijieapi.com /
bytedance.com / volcengine.com / volces.com / treecode.com 等 TRAE 相关域），
按账号(从 Authorization JWT 的 data.id 取)注入各自独立的伪 x-device-id /
x-market-user-id / vscode-sessionid，从而绕过"每设备每天"签到配额。

仅对签到接口 /trae/api/v2/ug/checkin_credits/claim 改写请求头，其余流量原样转发。
首次运行会生成自签根 CA (certs/ca.crt + ca.cer)，需管理员安装到
Windows 受信任根证书颁发机构，TRAE 才会信任代理证书。

环境变量：
    PROXY_PORT        监听端口（默认 8899）
    AUTO_CAPTURE_JWT  是否自动捕获写回 accounts.json（默认 1）
    TRAEDATA_DIR      数据目录（默认脚本所在目录）；应用通过此变量指向 %APPDATA%\TraeWorkAssistant

用法：
    python device_proxy.py            # 监听 127.0.0.1:8899
    python device_proxy.py --gen-ca   # 仅生成 CA 证书后退出（供安装流程调用）
    python device_proxy.py --capture-local   # 从 TRAE 本地 Cookies 解密提取 JWT 写回 accounts.json(仅 Windows)
"""
import os
import sys
import re
import json
import ssl
import socket
import shutil
import threading
import base64
import random
import uuid
import hashlib
import datetime
import http.client
import sqlite3
import gzip
import zlib

# ---------------- 配置 ----------------
LISTEN_HOST = "127.0.0.1"
LISTEN_PORT = int(os.environ.get("PROXY_PORT", "8899"))
BASE = os.path.dirname(os.path.abspath(__file__))
# 数据目录：优先 TRAEDATA_DIR（由桌面端注入），否则回退到脚本目录（保持独立可用性）
DATA_DIR = os.environ.get("TRAEDATA_DIR", BASE)
CONF_DIR = os.path.join(DATA_DIR, "conf")
DATA_SUBDIR = os.path.join(DATA_DIR, "data")
MAP_FILE = os.path.join(DATA_SUBDIR, "device_map.json")
LOG_FILE = os.path.join(DATA_DIR, "logs", "proxy.log")

PROXY_LOG_DIR = os.environ.get("PROXY_LOG_PATH", "")
if not PROXY_LOG_DIR:
    PROXY_LOG_DIR = os.path.join(DATA_DIR, "logs")

# 上游代理（可选）：桌面端在启动系统代理前会读取「已有的系统代理」（通常是用户的
# VPN 梯子，如 Clash/v2rayN 的本地代理），并将其作为上游透传进来。这样本代理只
# 拦截并解密 Trae 域名，其余流量（google/github 等）经上游 VPN 出去，避免「开代理后
# 外网打不开」的冲突。格式兼容 Windows 系统代理 ProxyServer：
#   "127.0.0.1:7890" / "http=127.0.0.1:7890" / "socks=127.0.0.1:7891"
#   / "http=127.0.0.1:7890;https=127.0.0.1:7890;socks=127.0.0.1:7891"
UPSTREAM_PROXY = os.environ.get("UPSTREAM_PROXY", "").strip()

def extract_sse_summary(resp_headers, resp_body):
    """从 SSE 流式响应中提取摘要信息（模型、token 用量等）。
    仅对 Content-Type: text/event-stream 的响应生效。
    返回 dict 或 None。"""
    if not resp_body:
        return None
    # 检查响应头是否为 SSE
    is_sse = False
    if resp_headers:
        for k, v in resp_headers:
            if k.lower() == "content-type" and "event-stream" in v.lower():
                is_sse = True
                break
    if not is_sse:
        return None
    body_str = resp_body.decode("utf-8", "replace") if isinstance(resp_body, bytes) else str(resp_body)
    summary = {}
    output_count = 0
    current_event = ""
    for line in body_str.split("\n"):
        line = line.strip()
        if line.startswith("event:"):
            current_event = line[6:].strip()
        elif line.startswith("data:"):
            data_str = line[5:].strip()
            if not data_str:
                continue
            try:
                data = json.loads(data_str)
            except Exception:
                continue
            if current_event == "metadata":
                model = data.get("model") or data.get("model_name")
                if model:
                    summary["model"] = model
                session_id = data.get("session_id")
                if session_id:
                    summary["session_id"] = session_id[:16] + "..."
            elif current_event == "output":
                output_count += 1
            elif current_event == "token_usage":
                pt = data.get("prompt_tokens")
                ct = data.get("completion_tokens")
                tt = data.get("total_tokens")
                if pt is not None:
                    summary["prompt_tokens"] = pt
                if ct is not None:
                    summary["completion_tokens"] = ct
                if tt is not None:
                    summary["total_tokens"] = tt
            elif current_event == "done":
                fr = data.get("finish_reason")
                if fr:
                    summary["finish_reason"] = fr
    if output_count > 0:
        summary["output_chunks"] = output_count
    return summary if summary else None


def decompress_body(body, resp_headers):
    """根据 Content-Encoding 头解压响应体，用于日志展示。
    返回解压后的 bytes；如解压失败则返回原始 body。"""
    if not body or not resp_headers:
        return body
    # resp_headers 可能是 list of tuples 或 dict
    encoding = None
    if isinstance(resp_headers, dict):
        encoding = resp_headers.get("Content-Encoding") or resp_headers.get("content-encoding")
    else:
        for k, v in resp_headers:
            if k.lower() == "content-encoding":
                encoding = v
                break
    if not encoding:
        return body
    encoding = encoding.lower().strip()
    try:
        if encoding == "gzip":
            return gzip.decompress(body)
        elif encoding == "deflate":
            # raw deflate 或 zlib-wrapped deflate
            try:
                return zlib.decompress(body)
            except zlib.error:
                return zlib.decompress(body, -zlib.MAX_WBITS)
        elif encoding == "br":
            # brotli：尝试导入 brotli 库，不可用时跳过
            try:
                import brotli
                return brotli.decompress(body)
            except ImportError:
                return body  # 无法解压，返回原始字节
    except Exception:
        pass
    return body


class ProxyRequestLogger:
    """将代理抓取到的完整请求/响应记录到明文文件，按 100MB 滚动存储。
    文件存放在 logs/ 目录下，文件名前缀 proxy_req_ 以区分 proxy.log 操作日志。"""
    def __init__(self, log_dir, max_size_mb=100):
        self.log_dir = log_dir
        self.max_size = max_size_mb * 1024 * 1024
        self._lock = threading.Lock()
        self._fd = None
        self._file_size = 0
        os.makedirs(log_dir, exist_ok=True)

    def _ensure_file(self):
        if self._fd and self._file_size < self.max_size:
            return
        if self._fd:
            self._fd.close()
        ts = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
        path = os.path.join(self.log_dir, f"proxy_req_{ts}.log")
        self._fd = open(path, "a", encoding="utf-8")
        self._file_size = os.path.getsize(path) if os.path.exists(path) else 0

    def log_request(self, method, host, path, req_headers, req_body, resp_status, resp_reason, resp_headers, resp_body):
        with self._lock:
            self._ensure_file()
            ts = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
            lines = [
                f"\n{'='*80}",
                f"[{ts}] {method} {host}{path}",
                f"--- Request Headers ---",
            ]
            for k, v in req_headers.items():
                lines.append(f"  {k}: {v}")
            if req_body:
                body_preview = req_body[:4096].decode("utf-8", "replace") if isinstance(req_body, bytes) else str(req_body)[:4096]
                lines.append(f"--- Request Body ({len(req_body)} bytes) ---")
                lines.append(body_preview)
            lines.append(f"--- Response: {resp_status} {resp_reason} ---")
            if resp_headers:
                for k, v in resp_headers:
                    lines.append(f"  {k}: {v}")
            if resp_body:
                # 先解压再展示，避免 gzip/deflate/br 压缩导致的乱码
                decompressed = decompress_body(resp_body, resp_headers)
                body_preview = decompressed[:8192].decode("utf-8", "replace") if isinstance(decompressed, bytes) else str(decompressed)[:8192]
                lines.append(f"--- Response Body ({len(resp_body)} bytes, decompressed {len(decompressed)} bytes) ---")
                lines.append(body_preview)
            # SSE 流摘要：对 llm_utils_chat 请求解析 SSE 事件
            sse_summary = extract_sse_summary(resp_headers, resp_body)
            if sse_summary:
                lines.append(f"--- SSE Summary ---")
                for k, v in sse_summary.items():
                    lines.append(f"  {k}: {v}")
            data = "\n".join(lines) + "\n"
            self._fd.write(data)
            self._fd.flush()
            self._file_size += len(data.encode("utf-8"))

    def log_websocket(self, host, path, req_headers):
        with self._lock:
            self._ensure_file()
            ts = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
            lines = [
                f"\n{'='*80}",
                f"[{ts}] [WebSocket Upgrade] {host}{path}",
                f"--- Request Headers ---",
            ]
            for k, v in req_headers.items():
                lines.append(f"  {k}: {v}")
            lines.append("--- WebSocket tunnel established (bidirectional, content not logged) ---")
            data = "\n".join(lines) + "\n"
            self._fd.write(data)
            self._fd.flush()
            self._file_size += len(data.encode("utf-8"))

_proxy_logger = None
def get_proxy_logger():
    global _proxy_logger
    if _proxy_logger is None:
        try:
            _proxy_logger = ProxyRequestLogger(PROXY_LOG_DIR)
            log(f"代理请求日志目录: {PROXY_LOG_DIR}")
        except Exception as e:
            log(f"代理请求日志初始化失败: {e}")
    return _proxy_logger

CA_DIR = os.path.join(DATA_SUBDIR, "certs")
SIGNIN_PATH = "/trae/api/v2/ug/checkin_credits/claim"
STATUS_PATH = "/trae/api/v2/ug/checkin_credits/status"
ACCOUNTS_FILE = os.path.join(DATA_SUBDIR, "checkin_accounts.json")
# 默认开启自动捕获 JWT 写回 checkin_accounts.json；设 AUTO_CAPTURE_JWT=0 关闭
AUTO_CAPTURE_JWT = os.environ.get("AUTO_CAPTURE_JWT", "1") not in ("0", "false", "False", "")

# 鉴权头嗅探去重集合：每个 host 仅提示一次，避免刷屏（用于诊断"代理是否见到鉴权头"）
_seen_auth_hints = set()

# TRAE 相关域名（后缀匹配）：仅用于日志分类与启动横幅，明确代理覆盖的全部监听域。
# 注意：JWT 捕获仍保持「不限 host」以保证鲁棒性（兼容未列出的子域 / 未来新域名，
# 例如实测出现的 trae-api-cn.mchost.gury），这里的清单只做可见性标记，不放宽也不收窄捕获。
TARGET_DOMAINS = [
    "trae.cn", "trae.com.cn", "mchost.guru",
    "zijieapi.com", "bytedance.com", "volcengine.com", "volces.com", "treecode.com",
]
# 支持从环境变量覆盖域名列表（桌面端设置页可配置）
_env_domains = os.environ.get("PROXY_DOMAINS", "")
if _env_domains:
    TARGET_DOMAINS = [d.strip() for d in _env_domains.split(",") if d.strip()]
# 已知 TRAE 接口（按 path 子串匹配），在日志中标出接口用途，便于一眼看清「监听接口」
KNOWN_TRAE_PATHS = [
    ("/trae/api/v2/ug/checkin_credits/claim", "签到领取"),
    ("/trae/api/v2/ug/checkin_credits/status", "签到状态"),
    ("/cloudide/api", "CloudIDE网关"),
    ("/oauth/", "OAuth"),
    ("ExchangeToken", "换Token"),
    ("ide_user_ent_usage", "积分用量"),
    ("/api/agent/v3/llm_utils_chat", "大模型对话"),
    ("/api/ide/v1/get_detail_param", "模型列表"),
    ("/api/remote/v1/plugins", "插件接口"),
    ("/api/remote/v1/skills", "技能列表"),
]


def host_in_targets(host):
    h = (host or "").lower()
    return any(h == d or h.endswith("." + d) for d in TARGET_DOMAINS)


def classify_path(path):
    p = path or ""
    for sub, name in KNOWN_TRAE_PATHS:
        if sub in p:
            return name
    return None

# ---------------- 日志 ----------------
_log_lock = threading.Lock()
os.makedirs(os.path.dirname(LOG_FILE), exist_ok=True)
_logf = open(LOG_FILE, "a", encoding="utf-8", buffering=1)

def log(*a):
    ts = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    line = f"[{ts}] " + " ".join(str(x) for x in a)
    with _log_lock:
        print(line, flush=True)
        try:
            _logf.write(line + "\n")
        except Exception:
            pass

# ---------------- 每账号设备映射 ----------------
_map_lock = threading.Lock()
_device_map = {}

def load_map():
    global _device_map
    if os.path.exists(MAP_FILE):
        try:
            with open(MAP_FILE, "r", encoding="utf-8") as f:
                _device_map = json.load(f)
        except Exception as e:
            log("读取映射失败:", e)
            _device_map = {}
    log(f"已加载设备映射: {len(_device_map)} 个账号")

def save_map():
    try:
        os.makedirs(os.path.dirname(MAP_FILE), exist_ok=True)
        with open(MAP_FILE, "w", encoding="utf-8") as f:
            json.dump(_device_map, f, ensure_ascii=False, indent=2)
    except Exception as e:
        log("保存映射失败:", e)

def _normalize_seed(seed):
    """将任意 seed 归一为稳定 int（与 auto_checkin.py 完全一致）。"""
    if isinstance(seed, int):
        return seed & 0x7FFFFFFFFFFFFFFF
    if isinstance(seed, str):
        if seed.isdigit():
            return int(seed) & 0x7FFFFFFFFFFFFFFF
        return int(hashlib.sha256(seed.encode("utf-8")).hexdigest(), 16) & 0x7FFFFFFFFFFFFFFF
    return seed


def _stable_rng(seed):
    """基于 seed 的稳定随机数生成器（与 auto_checkin.py 保持一致）。"""
    return random.Random(_normalize_seed(seed))


def _seeded_stream(seed, salt, nbytes):
    """确定性派生均匀字节流（SHA-256）。

    替代 random.Random(seed).choice —— 后者在某些 seed 下会产出
    连续相同字符的病态序列（如 uid=4487568582777872 时 device_id 全 '2'、
    session_id 全 '5' 的假占位符）。SHA-256 派生保证均匀且无病态，
    同时仍由 seed 决定，保证同一 user_id 始终得到同一设备标识。
    """
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
    bs[6] = (bs[6] & 0x0F) | 0x40  # version 4
    bs[8] = (bs[8] & 0x3F) | 0x80  # variant RFC 4122
    return str(uuid.UUID(bytes=bytes(bs)))


DEVICE_GEN = 2  # 设备标识生成算法版本；旧记录(gen 缺失=1)会自动重建


def get_device_for(user_id):
    user_id = str(user_id)
    with _map_lock:
        rec = _device_map.get(user_id)
        if rec is None or rec.get("gen", 1) < DEVICE_GEN:
            _device_map[user_id] = {
                "device_id": rand_digits(15, seed=user_id),
                "market_user_id": gen_market_uuid(user_id),
                "session_id": rand_hex(64, seed=user_id),
                "created": datetime.datetime.now().isoformat(timespec="seconds"),
                "gen": DEVICE_GEN,
            }
            save_map()
            log(f"  >> 新账号注册设备: user={user_id} -> x-device-id={_device_map[user_id]['device_id']}")
        return _device_map[user_id]

# ---------------- JWT user id 提取(不校验签名) ----------------
def extract_user_id(auth_header):
    if not auth_header:
        return None
    parts = auth_header.split(None, 1)
    token = parts[1] if len(parts) == 2 else parts[0]
    segs = token.split(".")
    if len(segs) < 2:
        return None
    try:
        pad = segs[1] + "=" * (-len(segs[1]) % 4)
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


def get_jwt_exp(jwt_full):
    """从 JWT 解 exp 字段返回 (datetime, None)；解析失败返回 (None, None)。"""
    if not jwt_full:
        return None, None
    token = jwt_full
    if token.startswith("Cloud-IDE-JWT "):
        token = token.split(None, 1)[1]
    segs = token.split(".")
    if len(segs) < 2:
        return None, None
    try:
        pad = segs[1] + "=" * (-len(segs[1]) % 4)
        payload = json.loads(base64.urlsafe_b64decode(pad))
        exp = payload.get("exp")
        if not isinstance(exp, (int, float)):
            return None, None
        return datetime.datetime.fromtimestamp(exp), exp
    except Exception:
        return None, None


# ---------------- checkin_accounts.json 捕获写回 ----------------
_accounts_lock = threading.RLock()  # 用 RLock 以便 update_account_jwt 持锁后再调 save_accounts
_accounts_cache = None
_accounts_mtime = None


def load_accounts(force_reload=False):
    """加载 checkin_accounts.json；带 mtime 检测，文件被外部改动时自动 reload。"""
    global _accounts_cache, _accounts_mtime
    with _accounts_lock:
        if not os.path.exists(ACCOUNTS_FILE):
            _accounts_cache = {"accounts": []}
            _accounts_mtime = 0
            return _accounts_cache
        try:
            mtime = os.path.getmtime(ACCOUNTS_FILE)
        except OSError:
            mtime = 0
        if not force_reload and _accounts_cache is not None and _accounts_mtime == mtime:
            return _accounts_cache
        try:
            with open(ACCOUNTS_FILE, "r", encoding="utf-8") as f:
                _accounts_cache = json.load(f)
            _accounts_mtime = mtime
            log(f"  [accounts] 重新加载 {len(_accounts_cache.get('accounts', []))} 个账号")
        except Exception as e:
            log(f"  [accounts] 读取失败: {e}")
            _accounts_cache = {"accounts": []}
            _accounts_mtime = mtime
        return _accounts_cache


def save_accounts():
    """把内存中的 accounts 写回磁盘（持锁原子替换）。"""
    global _accounts_cache, _accounts_mtime
    with _accounts_lock:
        if _accounts_cache is None:
            return
        os.makedirs(os.path.dirname(ACCOUNTS_FILE), exist_ok=True)
        tmp = ACCOUNTS_FILE + ".tmp"
        try:
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(_accounts_cache, f, ensure_ascii=False, indent=2)
            os.replace(tmp, ACCOUNTS_FILE)
            _accounts_mtime = os.path.getmtime(ACCOUNTS_FILE)
        except Exception as e:
            log(f"  [accounts] 写入失败: {e}")


def update_account_jwt(user_id, jwt_full):
    """
    按 user_id 查找账号，更新 jwt 字段。规则：
      - 新 JWT 的 exp 必须 ≥ 旧 JWT 的 exp，否则跳过（防止覆盖更新的 token）
      - 找不到对应账号 → 追加为 auto_<user_id前8位> 新账号
    返回 'updated' / 'appended' / 'skipped' / 'unchanged'
    """
    global _accounts_cache
    cfg = load_accounts()
    new_exp_dt, new_exp_ts = get_jwt_exp(jwt_full)
    new_exp_str = new_exp_dt.strftime("%Y-%m-%d %H:%M") if new_exp_dt else "?"
    with _accounts_lock:
        accounts = cfg.get("accounts", [])
        target = None
        for a in accounts:
            if str(a.get("UserID", "")) == str(user_id):
                target = a
                break
        if target:
            old_exp_dt, _ = get_jwt_exp(target.get("jwt", ""))
            if target.get("jwt") == jwt_full:
                return "unchanged"
            # 防降级：新 token 过期时间 ≤ 旧的 → 跳过
            if old_exp_dt and new_exp_dt and new_exp_dt <= old_exp_dt:
                old_exp_str = old_exp_dt.strftime("%Y-%m-%d %H:%M")
                log(f"  [JWT 跳过(更旧)] user={user_id} 账号={target.get('name', '?')} "
                    f"旧 exp={old_exp_str} 新 exp={new_exp_str}")
                return "skipped"
            target["jwt"] = jwt_full
            target["updated_at"] = datetime.datetime.now().isoformat(timespec="seconds")
            log(f"  [JWT 自动更新] user={user_id} 账号={target.get('name', '?')} exp={new_exp_str}")
            save_accounts()
            return "updated"
        # 新账号
        new_acc = {
            "name": f"auto_{str(user_id)[:8]}",
            "UserID": user_id,
            "jwt": jwt_full,
            "added_at": datetime.datetime.now().isoformat(timespec="seconds"),
        }
        accounts.append(new_acc)
        log(f"  [JWT 自动追加新账号] user={user_id} -> name={new_acc['name']} exp={new_exp_str}")
        save_accounts()
        return "appended"


def update_account_refresh_token(user_id, refresh_token):
    """按 user_id 查找账号，更新 refresh_token 字段。
    仅当新 token 非空且与旧值不同时才写盘。返回 'updated' / 'not_found' / 'unchanged'。"""
    cfg = load_accounts()
    with _accounts_lock:
        accounts = cfg.get("accounts", [])
        target = None
        for a in accounts:
            if str(a.get("UserID", "")) == str(user_id):
                target = a
                break
        if not target:
            return "not_found"
        if target.get("refresh_token") == refresh_token:
            return "unchanged"
        target["refresh_token"] = refresh_token
        target["refresh_token_updated_at"] = datetime.datetime.now().isoformat(timespec="seconds")
        log(f"  [refresh_token 更新] user={user_id} 账号={target.get('name', '?')}")
        save_accounts()
        return "updated"


def try_capture_refresh_token_from_response(host, path, resp_body):
    """从 ExchangeToken 响应体中提取 refresh_token 并写回 accounts.json。
    ExchangeToken 响应格式（本项目协议分析）：
    {"code":0,"data":{"access_token":"<jwt>","refresh_token":"<rt>",...}}
    或 {"code":0,"data":{"token":"<jwt>","refresh_token":"<rt>",...}}
    """
    if not resp_body or b"refresh_token" not in resp_body:
        return
    try:
        body_str = resp_body.decode("utf-8", "replace")
        data = json.loads(body_str)
    except Exception:
        return
    inner = data.get("data", data) if isinstance(data, dict) else None
    if not isinstance(inner, dict):
        return
    rt = inner.get("refresh_token")
    if not rt or not isinstance(rt, str):
        return
    # 尝试从 access_token/token 提取 user_id
    at = inner.get("access_token") or inner.get("token") or ""
    if at:
        validated = _valid_cloud_ide_jwt(at) if not at.startswith("Cloud-IDE-JWT") else at
        if not validated:
            validated = _valid_cloud_ide_jwt(at)
        if validated:
            uid = extract_user_id(validated)
            if uid:
                update_account_jwt(uid, validated)  # 同时更新 accessToken
                update_account_refresh_token(uid, rt)
                return
    # 如果响应中没有 access_token，尝试用请求中的 user_id（由调用方注入）
    log(f"  [refresh_token] 响应中含 refresh_token 但无法提取 user_id，跳过")


# ---------------- 本地 Cookies 解密捕获 JWT (仅 Windows) ----------------
# 背景：当前版本 TRAE 的鉴权请求(api.trae.cn)不走 Chromium `--proxy-server` 代理，
# MITM 代理抓不到 Cloud-IDE-JWT。但 TRAE 把登录态存在本地 Cookies(Chromium 格式)，
# 此处直接在 Windows 侧解密提取并写回 checkin_accounts.json，作为代理方案的兜底。
def _trae_app_dir():
    # TRAE SOLO CN 用户数据目录；可用 TRAE_APP_DIR 覆盖
    return os.environ.get("TRAE_APP_DIR") or os.path.join(
        os.environ.get("APPDATA", ""), "TRAE SOLO CN")


def _chrome_aes_key(app_dir):
    """读 Local State -> os_crypt.encrypted_key(DPAPI 包裹的 AES-256 密钥)。"""
    lp = os.path.join(app_dir, "Local State")
    if not os.path.exists(lp):
        return None
    try:
        with open(lp, "r", encoding="utf-8") as f:
            state = json.load(f)
        b64 = state.get("os_crypt", {}).get("encrypted_key")
        if not b64:
            return None
        raw = base64.b64decode(b64)
        if raw[:5] != b"DPAPI":
            return None
        import win32crypt  # 仅 Windows
        return win32crypt.CryptUnprotectData(raw[5:], None, None, None, 0)[1]
    except Exception as e:
        log("  [local] 读取 Chromium AES 密钥失败:", e)
        return None


def _decrypt_cookie(enc, key):
    if not enc:
        return None
    try:
        if enc[:3] == b"v10":  # 现代 Chromium: AES-256-GCM
            from cryptography.hazmat.primitives.ciphers.aead import AESGCM
            iv, ct = enc[3:15], enc[15:]
            return AESGCM(key).decrypt(iv, ct, None).decode("utf-8", "replace")
        import win32crypt  # 旧格式：直接 DPAPI
        return win32crypt.CryptUnprotectData(bytes(enc), None, None, None, 0)[1].decode("utf-8", "replace")
    except Exception:
        return None


_JWT_RE = re.compile(r"[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+")


def _valid_cloud_ide_jwt(tok):
    """校验单个 JWT 是否为 Cloud-IDE-JWT：头部 alg=RS256 且 payload 含 data.id。
    满足则返回 'Cloud-IDE-JWT <jwt>'，否则 None。（签名不校验，仅看声明）"""
    parts = tok.split(".")
    if len(parts) != 3:
        return None
    try:
        h = json.loads(base64.urlsafe_b64decode(parts[0] + "=" * (-len(parts[0]) % 4)))
        p = json.loads(base64.urlsafe_b64decode(parts[1] + "=" * (-len(parts[1]) % 4)))
    except Exception:
        return None
    if h.get("alg") == "RS256" and isinstance(p.get("data"), dict) and p["data"].get("id"):
        return "Cloud-IDE-JWT " + tok
    return None


def _find_cloud_ide_jwt(blob):
    """从一段文本里找 Cloud-IDE-JWT。返回 'Cloud-IDE-JWT <jwt>' 或 None。"""
    if not blob:
        return None
    if isinstance(blob, bytes):
        blob = blob.decode("utf-8", "replace")
    m = re.search(r"Cloud-IDE-JWT\s+([A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+)", blob)
    if m:
        return _valid_cloud_ide_jwt(m.group(1))
    for m in _JWT_RE.finditer(blob):
        r = _valid_cloud_ide_jwt(m.group(0))
        if r:
            return r
    return None


def capture_from_local():
    """解密 TRAE 本地 Cookies + 扫描 Local Storage，提取 Cloud-IDE-JWT 写回 accounts.json。
    仅 Windows 有效(需 win32crypt + cryptography)。返回新增/更新账号数。"""
    if AUTO_CAPTURE_JWT:
        load_accounts()
    app_dir = _trae_app_dir()
    log(f"[local] TRAE 数据目录: {app_dir}")
    if not os.path.isdir(app_dir):
        log("[local] 目录不存在，跳过")
        return 0
    key = _chrome_aes_key(app_dir)
    if key is None:
        log("[local] 未取得 AES 密钥(需 Windows 已登录用户 + pywin32)；将仅扫描明文值")
    found = 0
    # 1) Cookies 数据库
    dbs = [os.path.join(app_dir, "Network", "Cookies")]
    tw = os.path.join(app_dir, "Partitions", "trae-webview", "Cookies")
    if os.path.exists(tw):
        dbs.append(tw)
    for db in dbs:
        if not os.path.exists(db):
            continue
        tmp = os.path.join(DATA_DIR, ".trae_cookies_scan.tmp")
        try:
            shutil.copy(db, tmp)
        except Exception:
            tmp = db
        try:
            con = sqlite3.connect(f"file:{tmp}?mode=ro")
            for host, name, value, enc in con.execute("SELECT host_key,name,value,encrypted_value FROM cookies"):
                blob = value or ""
                if enc:
                    d = _decrypt_cookie(enc, key) if key else None
                    if d is not None:
                        blob = d
                hit = _find_cloud_ide_jwt(blob)
                if hit:
                    uid = extract_user_id(hit)
                    if uid:
                        r = update_account_jwt(uid, hit)
                        log(f"[local] 命中 Cookies host={host} name={name} -> {r}")
                        found += 1
            con.close()
        except Exception as e:
            log(f"[local] 读取 Cookies 失败 {db}: {e}")
        finally:
            if tmp != db and os.path.exists(tmp):
                try:
                    os.remove(tmp)
                except Exception:
                    pass
    # 2) Local Storage leveldb 明文兜底扫描
    for ls in (os.path.join(app_dir, "Local Storage", "leveldb"),
               os.path.join(app_dir, "Partitions", "trae-webview", "Local Storage", "leveldb")):
        if not os.path.isdir(ls):
            continue
        for fn in os.listdir(ls):
            if not fn.endswith((".ldb", ".log")):
                continue
            try:
                with open(os.path.join(ls, fn), "rb") as f:
                    data = f.read()
            except Exception:
                continue
            for m in re.finditer(rb"Cloud-IDE-JWT [A-Za-z0-9_\-\.=]+", data):
                hit = m.group(0).decode("utf-8", "replace")
                uid = extract_user_id(hit)
                if uid:
                    r = update_account_jwt(uid, hit)
                    log(f"[local] 命中 leveldb {fn} -> {r}")
                    found += 1
    log(f"[local] 本地捕获完成，新增/更新 {found} 个账号")
    return found

# ---------------- CA / 叶子证书 ----------------
_ca_cert = _ca_key = None
_leaf_cache = {}
_LEAF_CACHE_MAX = 50  # 最多缓存 50 个域名的叶子证书，防止内存无限增长

def ensure_ca():
    global _ca_cert, _ca_key
    cert_pem = os.path.join(CA_DIR, "ca.crt")
    key_pem = os.path.join(CA_DIR, "ca.key")
    cer_der = os.path.join(CA_DIR, "ca.cer")
    if os.path.exists(cert_pem) and os.path.exists(key_pem):
        from cryptography.hazmat.primitives.serialization import load_pem_private_key
        from cryptography import x509
        with open(cert_pem, "rb") as f:
            _ca_cert = x509.load_pem_x509_certificate(f.read())
        with open(key_pem, "rb") as f:
            _ca_key = load_pem_private_key(f.read(), password=None)
        log("已加载现有 CA:", cert_pem)
        return
    from cryptography import x509
    from cryptography.x509.oid import NameOID
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import rsa
    os.makedirs(CA_DIR, exist_ok=True)
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    subj = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "TraeDeviceProxyCA")])
    cert = (
        x509.CertificateBuilder()
        .subject_name(subj)
        .issuer_name(subj)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(days=1))
        .not_valid_after(datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=3650))
        .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
        .add_extension(
            x509.KeyUsage(
                digital_signature=True, content_commitment=False, key_encipherment=False,
                data_encipherment=False, key_agreement=False, key_cert_sign=True,
                crl_sign=True, encipher_only=False, decipher_only=False,
            ),
            critical=True,
        )
        .sign(key, hashes.SHA256())
    )
    with open(cert_pem, "wb") as f:
        f.write(cert.public_bytes(serialization.Encoding.PEM))
    with open(key_pem, "wb") as f:
        f.write(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.TraditionalOpenSSL, serialization.NoEncryption()))
    with open(cer_der, "wb") as f:
        f.write(cert.public_bytes(serialization.Encoding.DER))
    _ca_cert, _ca_key = cert, key
    log("已生成自签 CA ->", cert_pem, "/", cer_der)

def leaf_cert(host):
    if host in _leaf_cache:
        # LRU: 移到末尾（Python 3.7+ dict 保持插入顺序，删除再插入即为 LRU 更新）
        val = _leaf_cache.pop(host)
        _leaf_cache[host] = val
        return val
    from cryptography import x509
    from cryptography.x509.oid import NameOID
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import rsa
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    cert = (
        x509.CertificateBuilder()
        .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, host)]))
        .issuer_name(_ca_cert.subject)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(days=1))
        .not_valid_after(datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=3650))
        .add_extension(x509.SubjectAlternativeName([x509.DNSName(host)]), critical=False)
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .sign(_ca_key, hashes.SHA256())
    )
    cpath = os.path.join(CA_DIR, f"leaf_{host}.crt")
    kpath = os.path.join(CA_DIR, f"leaf_{host}.key")
    with open(cpath, "wb") as f:
        f.write(cert.public_bytes(serialization.Encoding.PEM))
    with open(kpath, "wb") as f:
        f.write(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.TraditionalOpenSSL, serialization.NoEncryption()))
    # LRU 驱逐：超过上限时删除最旧的条目
    if len(_leaf_cache) >= _LEAF_CACHE_MAX:
        oldest = next(iter(_leaf_cache))
        del _leaf_cache[oldest]
        log(f"  [leaf_cert] LRU 驱逐: {oldest}")
    _leaf_cache[host] = (cpath, kpath)
    return cpath, kpath

# ---------------- HTTP 请求/响应读写 ----------------
def recv_until(sock, terminator, buf=b"", max_size=10 * 1024 * 1024):
    """读取直到遇到 terminator，限制最大 10MB 防止 OOM"""
    while terminator not in buf:
        if len(buf) > max_size:
            log(f"  [recv_until] 缓冲区超限 ({len(buf)} > {max_size})，截断")
            return None
        chunk = sock.recv(4096)
        if not chunk:
            return None
        buf += chunk
    return buf

def read_http_request(sock):
    buf = recv_until(sock, b"\r\n\r\n")
    if buf is None:
        return None
    header_blob, _, rest = buf.partition(b"\r\n\r\n")
    lines = header_blob.split(b"\r\n")
    first = lines[0].decode("utf-8", "replace")
    parts = first.split(" ")
    method = parts[0]
    path = parts[1] if len(parts) > 1 else "/"
    version = parts[2] if len(parts) > 2 else "HTTP/1.1"
    headers = {}
    for line in lines[1:]:
        if b":" in line:
            k, _, v = line.partition(b":")
            headers[k.decode("utf-8", "replace").strip().lower()] = v.decode("utf-8", "replace").strip()
    cl = int(headers.get("content-length", 0) or 0)
    body = rest
    while len(body) < cl:
        chunk = sock.recv(4096)
        if not chunk:
            break
        body += chunk
    return method, path, version, headers, body[:cl] if cl else body

def send_response(sock, status, reason, headers, body):
    if isinstance(body, str):
        body = body.encode("utf-8")
    out = {}
    for k, v in headers.items():
        if k.lower() in ("transfer-encoding", "connection", "keep-alive"):
            continue
        out[k] = v
    out["Content-Length"] = str(len(body))
    out["Connection"] = "keep-alive"
    head = [f"HTTP/1.1 {status} {reason}".encode("utf-8")]
    for k, v in out.items():
        head.append(f"{k}: {v}".encode("utf-8"))
    sock.sendall(b"\r\n".join(head) + b"\r\n\r\n" + body)

HOP_BY_HOP = {"proxy-connection", "connection", "keep-alive", "proxy-authorization", "host", "content-length"}

def is_websocket_upgrade(headers):
    """检测 WebSocket 升级请求"""
    return headers.get("upgrade", "").lower() == "websocket"

def forward_websocket(host, port, method, path, headers, body, client_tls):
    """处理 WebSocket 升级：转发升级请求 -> 获取 101 响应 -> 双向原始隧道。"""
    ws_tag = f"[WebSocket] {host}:{port}{path}"
    try:
        # 1. 建立到上游的 TLS 连接
        log(f"  {ws_tag} 正在连接上游 {host}:{port}...")
        ctx = ssl.create_default_context()
        raw_sock = socket.create_connection((host, port), timeout=30)
        upstream_tls = ctx.wrap_socket(raw_sock, server_hostname=host)
        log(f"  {ws_tag} 上游 TLS 连接已建立")

        try:
            # 2. 构建并发送升级请求
            # 重要：WebSocket 握手必须保留 Connection / Upgrade / Host 头。
            # 原代码对全部请求套用 HOP_BY_HOP 过滤，会把 "Connection: Upgrade"
            # 一并去掉，导致上游按普通 HTTP 请求处理并返回 400 Bad Request，
            # Trae 的实时通道（含"排队提醒"等通知）因此彻底失效、客户端卡死等待。
            # 这里用 WS 专用的跳过集合，并对接头缺失项做兜底补齐。
            _WS_SKIP = {"proxy-connection", "proxy-authorization", "content-length", "keep-alive"}
            req_lines = [f"{method} {path} HTTP/1.1"]
            _seen = set()
            for k, v in headers.items():
                kl = k.lower()
                if kl in _WS_SKIP:
                    continue
                req_lines.append(f"{k}: {v}")
                _seen.add(kl)
            if "upgrade" not in _seen:
                req_lines.append("Upgrade: websocket")
            if "connection" not in _seen:
                req_lines.append("Connection: Upgrade")
            log(f"  {ws_tag} 转发升级请求头: " + " | ".join(
                f"{k}: {v}" for k, v in (h.split(": ", 1) for h in req_lines[1:] if ": " in h)))
            req_data = "\r\n".join(req_lines).encode("utf-8") + b"\r\n\r\n"
            if body:
                req_data += body
            log(f"  {ws_tag} 发送升级请求: {method} {path} ({len(req_data)} bytes)")
            log(f"  {ws_tag} 关键头: Upgrade={headers.get('upgrade','?')} Connection={headers.get('connection','?')} Sec-WebSocket-Key={headers.get('sec-websocket-key','?')[:16]}...")
            upstream_tls.sendall(req_data)

            # 3. 读取上游响应（应为 101 Switching Protocols）
            log(f"  {ws_tag} 等待上游响应...")
            resp_buf = b""
            while b"\r\n\r\n" not in resp_buf:
                chunk = upstream_tls.recv(4096)
                if not chunk:
                    log(f"  {ws_tag} 上游在响应前关闭了连接 (已收到 {len(resp_buf)} bytes)")
                    break
                resp_buf += chunk
            if not resp_buf:
                log(f"  {ws_tag} 上游响应为空，放弃")
                return

            # 4. 转发响应给客户端
            first_line = resp_buf.split(b"\r\n", 1)[0].decode("utf-8", "replace")
            log(f"  {ws_tag} 收到上游响应: {first_line} ({len(resp_buf)} bytes)")
            # 检查是否有额外的响应体数据（在 \r\n\r\n 之后的部分）
            header_end = resp_buf.find(b"\r\n\r\n")
            extra_body = resp_buf[header_end + 4:] if header_end >= 0 else b""
            client_tls.sendall(resp_buf)
            log(f"  {ws_tag} 响应已转发给客户端")

            # 5. 检查是否升级成功
            if " 101 " not in first_line:
                log(f"  {ws_tag} 升级失败: {first_line}")
                return

            log(f"  {ws_tag} 升级成功 (101 Switching Protocols), 开始双向隧道")
            # 记录到代理日志
            pl = get_proxy_logger()
            if pl:
                pl.log_websocket(host, path, headers)

            # 6. 双向隧道：client_tls <-> upstream_tls
            # 如果响应中包含额外的 body 数据，需要先把它发给对端
            if extra_body:
                log(f"  {ws_tag} 响应中包含 {len(extra_body)} bytes 额外数据, 已包含在转发中")

            tunnel_stats = {"client_to_up": 0, "up_to_client": 0, "closed_by": ""}

            def _pipe(src, dst, direction, stats):
                try:
                    while True:
                        data = src.recv(65536)
                        if not data:
                            log(f"  {ws_tag} {direction} 连接关闭 (已转发 {stats[direction]} bytes)")
                            stats["closed_by"] = direction
                            break
                        stats[direction] += len(data)
                        dst.sendall(data)
                except Exception as e:
                    log(f"  {ws_tag} {direction} 隧道异常: {type(e).__name__}: {e}")
                    stats["closed_by"] = direction + "_error"
                finally:
                    try:
                        dst.shutdown(socket.SHUT_WR)
                    except Exception:
                        pass

            t1 = threading.Thread(target=_pipe, args=(client_tls, upstream_tls, "client_to_up", tunnel_stats), daemon=True)
            t2 = threading.Thread(target=_pipe, args=(upstream_tls, client_tls, "up_to_client", tunnel_stats), daemon=True)
            t1.start()
            t2.start()
            t1.join()
            t2.join()
            log(f"  {ws_tag} 双向隧道结束: client->up={tunnel_stats['client_to_up']} bytes, up->client={tunnel_stats['up_to_client']} bytes, 关闭方={tunnel_stats['closed_by']}")

        except Exception as e:
            log(f"  {ws_tag} 升级过程异常: {type(e).__name__}: {e}")
        finally:
            try:
                upstream_tls.close()
                log(f"  {ws_tag} 上游连接已关闭")
            except Exception:
                pass
    except Exception as e:
        log(f"  {ws_tag} 连接上游失败: {type(e).__name__}: {e}")

def _stream_response(resp, resp_headers, client_sock, host, method, path, req_headers, req_body):
    """流式转发 SSE 响应，逐块读取并立即发送给客户端，避免全量缓冲导致超时。"""
    resp_status = resp.status
    resp_reason = resp.reason

    head = [f"HTTP/1.1 {resp_status} {resp_reason}".encode("utf-8")]
    for k, v in resp_headers.items():
        if k.lower() in ("transfer-encoding", "connection", "keep-alive", "content-length"):
            continue
        head.append(f"{k}: {v}".encode("utf-8"))
    head.append(b"Transfer-Encoding: chunked")
    head.append(b"Connection: keep-alive")
    client_sock.sendall(b"\r\n".join(head) + b"\r\n\r\n")

    log(f"  [forward] <- {resp_status} {resp_reason} (streaming) from {host}{path}")

    total_bytes = 0
    logged_body = bytearray()
    MAX_LOG_BODY = 10 * 1024 * 1024
    interrupted = False

    try:
        while True:
            chunk = resp.read(8192)
            if not chunk:
                break
            client_sock.sendall(f"{len(chunk):x}\r\n".encode("ascii") + chunk + b"\r\n")
            total_bytes += len(chunk)
            if len(logged_body) < MAX_LOG_BODY:
                logged_body.extend(chunk)
        client_sock.sendall(b"0\r\n\r\n")
    except Exception as e:
        interrupted = True
        log(f"  [forward] 流式转发中断: {type(e).__name__}: {e} (已转发 {total_bytes} bytes)")
        try:
            client_sock.sendall(b"0\r\n\r\n")
        except Exception:
            pass

    log(f"  [forward] 流式转发{'中断' if interrupted else '完成'}，共 {total_bytes} bytes")

    pl = get_proxy_logger()
    if pl:
        pl.log_request(method, host, path, req_headers, req_body, resp_status, resp_reason, resp.getheaders(), bytes(logged_body))

    return not interrupted


def forward_upstream(host, port, method, path, headers, body, client_sock):
    log(f"  [forward] -> {method} https://{host}:{port}{path} ({len(body) if body else 0} bytes body)")
    ctx = ssl.create_default_context()
    # 检测是否为流式请求（SSE）：流式请求使用更长超时
    is_stream = "llm_utils_chat" in path or "event-stream" in headers.get("accept", "")
    timeout = 300 if is_stream else 30
    conn = http.client.HTTPSConnection(host, port, context=ctx, timeout=timeout)
    fwd = {k: v for k, v in headers.items() if k.lower() not in HOP_BY_HOP}
    try:
        body_arg = body if method.upper() != "GET" else None
        conn.request(method, path, body=body_arg, headers=fwd)
        resp = conn.getresponse()
        resp_headers = dict(resp.getheaders())

        if is_stream:
            return _stream_response(resp, resp_headers, client_sock, host, method, path, headers, body)

        resp_body = resp.read()
        log(f"  [forward] <- {resp.status} {resp.reason} ({len(resp_body)} bytes) from {host}{path}")
        upstream_conn = resp_headers.get("connection", "").lower()
        send_response(client_sock, resp.status, resp.reason, resp_headers, resp_body)
        # 捕获 ExchangeToken 响应中的 refresh_token
        if "ExchangeToken" in path or "oauth" in path.lower():
            try_capture_refresh_token_from_response(host, path, resp_body)
        # 记录到代理请求日志
        pl = get_proxy_logger()
        if pl:
            pl.log_request(method, host, path, headers, body, resp.status, resp.reason, resp.getheaders(), resp_body)
        # 如果上游要求关闭连接，返回 False 通知调用方退出 keep-alive 循环
        if upstream_conn == "close":
            return False
        return True
    except socket.timeout:
        log(f"  [forward] 超时 (timeout={timeout}s): {host}{path}")
        send_response(client_sock, 504, "Gateway Timeout", {}, b"Gateway Timeout")
        return False
    except Exception as e:
        log(f"  [forward] 错误: {type(e).__name__}: {e} (host={host}, path={path})")
        send_response(client_sock, 502, "Bad Gateway", {}, b"Bad Gateway")
        return False
    finally:
        conn.close()

# ---------------- 隧道(HTTPS over CONNECT) ----------------
def tunnel_https(tls, host, port):
    log(f"  [MITM] 进入 HTTPS 解密隧道: {host}:{port}")
    while True:
        try:
            req = read_http_request(tls)
        except Exception as e:
            log(f"  [MITM] 读取 TLS 请求错误: {type(e).__name__}: {e}")
            break
        if req is None:
            log(f"  [MITM] 客户端关闭连接: {host}:{port}")
            break
        method, path, version, headers, body = req
        tag = " [TRAE]" if host_in_targets(host) else ""
        ep = classify_path(path)
        ep_tag = f"  <{ep}>" if ep else ""
        log(f"  {method} {host}{path}{tag}{ep_tag}")
        # 鉴权头嗅探（仅诊断用）：见到任意鉴权头即打印一次类型，
        # 用于判断"代理是否真的见到 TRAE 的鉴权流量"。
        if AUTO_CAPTURE_JWT:
            _auth_val = None
            _auth_name = None
            for _h in ("authorization", "x-cloudide-token", "x-icube-token"):
                _v = headers.get(_h)
                if _v:
                    _auth_name = _h
                    _auth_val = _v.strip()
                    break
            if _auth_val:
                _k = f"auth:{host}"
                if _k not in _seen_auth_hints:
                    _seen_auth_hints.add(_k)
                    _raw_jwt = _auth_val[len("Cloud-IDE-JWT"):].strip() if _auth_val.startswith("Cloud-IDE-JWT") else _auth_val
                    _is_valid = bool(_valid_cloud_ide_jwt(_raw_jwt))
                    log(f"  [JWT-DEBUG] 在 {host} 发现 {_auth_name} 头 (类型: {'Cloud-IDE-JWT ✅可捕获' if _is_valid else '其他/不可识别: ' + _auth_val[:18] + '…'})")
        # 自动捕获 JWT：兼容 authorization: Cloud-IDE-JWT <jwt>、x-cloudide-token: <jwt>
        # 以及 x-icube-token: <jwt>。不限 host，覆盖 sign-in / status / 任意接口。
        if AUTO_CAPTURE_JWT:
            auth = None
            for _h in ("authorization", "x-cloudide-token", "x-icube-token"):
                _v = headers.get(_h)
                if _v:
                    auth = _v.strip()
                    break
            if auth:
                jwt_val = auth[len("Cloud-IDE-JWT"):].strip() if auth.startswith("Cloud-IDE-JWT") else auth
                validated = _valid_cloud_ide_jwt(jwt_val)
                if validated:
                    uid_c = extract_user_id(validated)
                    if uid_c:
                        update_account_jwt(uid_c, validated)
        # 设备头改写（仅签到接口）——兼容 authorization / x-cloudide-token / x-icube-token
        if SIGNIN_PATH in path:
            auth = None
            for _h in ("authorization", "x-cloudide-token", "x-icube-token"):
                _v = headers.get(_h)
                if _v:
                    auth = _v.strip()
                    break
            if auth and auth.startswith("Cloud-IDE-JWT"):
                auth = auth[len("Cloud-IDE-JWT"):].strip()
            uid = extract_user_id(auth)
            if uid:
                dev = get_device_for(uid)
                headers["x-device-id"] = dev["device_id"]
                headers["x-market-user-id"] = dev["market_user_id"]
                headers["vscode-sessionid"] = dev["session_id"]
                log(f"  [签到改写] user={uid} -> x-device-id={dev['device_id']} x-market-user-id={dev['market_user_id'][:8]}... vscode-sessionid={dev['session_id'][:8]}...")
            else:
                log("  [签到] 未解析到 user id，未改写")
        # WebSocket 升级检测：如果是 WS 请求则走专用通道
        if is_websocket_upgrade(headers):
            log(f"  [WebSocket] 检测到升级请求: {method} {host}:{port}{path} (Connection={headers.get('connection','?')})")
            forward_websocket(host, port, method, path, headers, body, tls)
            break  # WS 连接已接管，退出 tunnel_https 循环
        keep_alive = forward_upstream(host, port, method, path, headers, body, tls)
        if keep_alive is False:
            break  # 上游要求关闭连接或出错，退出 keep-alive 循环

# ---------------- 上游代理透传（解决与 VPN 梯子的冲突） ----------------

def _split_host_port(addr, default_port):
    """把 'host:port' 拆成 (host, port)；无端口时用 default_port。"""
    addr = (addr or "").strip()
    if ":" in addr:
        h, p = addr.rsplit(":", 1)
        try:
            return h, int(p)
        except ValueError:
            return addr, default_port
    return addr, default_port


def _parse_upstream(spec):
    """解析上游代理规格，返回 ('http', addr) / ('socks5', addr) / None。
    兼容 Windows 系统代理 ProxyServer 的多种写法：
      - '127.0.0.1:7890'              -> http
      - 'http=127.0.0.1:7890'         -> http
      - 'socks=127.0.0.1:7891'        -> socks5
      - 'http=...;https=...;socks=...' -> 优先 socks5，其次 http
    """
    spec = (spec or "").strip()
    if not spec:
        return None
    parts = [p.strip() for p in spec.split(";") if p.strip()]
    socks = None
    http = None
    for p in parts:
        if "=" in p:
            k, v = p.split("=", 1)
            k = k.strip().lower()
            v = v.strip()
            if k in ("socks", "socks5"):
                socks = v
            elif k in ("http", "https"):
                http = http or v
        else:
            http = http or p
    if socks:
        return ("socks5", socks)
    if http:
        return ("http", http)
    return None


def connect_via_upstream(host, port, upstream):
    """经上游代理建立到 (host, port) 的 TCP 隧道，返回已建连的 socket。
    支持 HTTP 代理的 CONNECT，以及 SOCKS5（无认证 / 用户名密码）。"""
    kind, addr = upstream
    uh, up = _split_host_port(addr, 1080 if kind == "socks5" else 8080)
    if kind == "http":
        s = socket.create_connection((uh, up), timeout=30)
        req = (
            f"CONNECT {host}:{port} HTTP/1.1\r\n"
            f"Host: {host}:{port}\r\n"
            f"Proxy-Connection: keep-alive\r\n\r\n"
        )
        s.sendall(req.encode("utf-8"))
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = s.recv(4096)
            if not chunk:
                raise RuntimeError("上游代理无响应")
            buf += chunk
        head = buf.split(b"\r\n", 1)[0].decode("utf-8", "replace")
        if not head.startswith("HTTP/1.1 200") and not head.startswith("HTTP/1.0 200"):
            raise RuntimeError(f"上游代理拒绝 CONNECT: {head}")
        return s
    # SOCKS5
    s = socket.create_connection((uh, up), timeout=30)
    s.sendall(b"\x05\x02\x00\x02")
    greet = s.recv(2)
    if len(greet) < 2 or greet[0] != 0x05:
        raise RuntimeError("SOCKS5 握手失败")
    method = greet[1]
    if method == 0x02:
        user = os.environ.get("UPSTREAM_PROXY_USER", "").encode("utf-8")[:255]
        pwd = os.environ.get("UPSTREAM_PROXY_PASS", "").encode("utf-8")[:255]
        s.sendall(b"\x01" + bytes([len(user)]) + user + bytes([len(pwd)]) + pwd)
        rep = s.recv(2)
        if len(rep) < 2 or rep[1] != 0x00:
            raise RuntimeError("SOCKS5 认证失败")
    elif method != 0x00:
        raise RuntimeError(f"SOCKS5 不支持的认证方式: {method}")
    if ":" in host:
        raise RuntimeError("暂不支持 SOCKS5 IPv6")
    host_b = host.encode("utf-8")
    s.sendall(b"\x05\x01\x00\x03" + bytes([len(host_b)]) + host_b + port.to_bytes(2, "big"))
    rep = s.recv(4)
    if len(rep) < 4 or rep[1] != 0x00:
        raise RuntimeError(f"SOCKS5 CONNECT 失败: code={rep[1] if len(rep) >= 2 else '?'}")
    atyp = rep[3]
    if atyp == 0x01:
        s.recv(4)
    elif atyp == 0x03:
        n = s.recv(1)[0]
        s.recv(n)
    elif atyp == 0x04:
        s.recv(16)
    return s


# ---------------- 透明隧道（非 TRAE 域，不解密直接放行，静默无日志） ----------------
def tunnel_raw(client_sock, host, port):
    """对不在 TARGET_DOMAINS 的 CONNECT，建立到真实服务器的 TCP 隧道并双向转发，
    不做 TLS 解密、不记录任何日志。用于把本代理作为系统代理时，让浏览器/其他 App
    的流量正常通过而不污染日志。

    关键修复：CONNECT 隧道必须先用 "HTTP/1.1 200 Connection Established" 应答客户端，
    客户端才会发起 TLS 握手。缺失该应答会导致 github.com 等所有非 Trae 域名的
    HTTPS 流量无法建立隧道（表现为「代理打开后网站打不开」）。详见问题分析报告。
    """
    upstream = _parse_upstream(UPSTREAM_PROXY) if UPSTREAM_PROXY else None
    remote = None
    if upstream:
        try:
            remote = connect_via_upstream(host, port, upstream)
            log(f"  [raw-tunnel] 经上游代理 {upstream[1]} 建立隧道 {host}:{port}")
        except Exception as e:
            log(f"  [raw-tunnel] 上游代理连接失败({type(e).__name__}: {e})，回退直连")
            remote = None
    if remote is None:
        try:
            remote = socket.create_connection((host, port), timeout=30)
        except Exception:
            # 上游不可达：明确告知客户端，避免浏览器无限等待
            try:
                client_sock.sendall(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            except Exception:
                pass
            return

    # 完成 CONNECT 握手：先回 200，客户端随后才会发送 TLS ClientHello
    try:
        client_sock.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
    except Exception:
        return

    _raw_stats = {"c2r": 0, "r2c": 0}

    def pipe(src, dst, direction, stats):
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                stats[direction] += len(data)
                dst.sendall(data)
        except Exception:
            pass
        finally:
            try:
                dst.shutdown(socket.SHUT_WR)
            except Exception:
                pass

    t1 = threading.Thread(target=pipe, args=(client_sock, remote, "c2r", _raw_stats), daemon=True)
    t2 = threading.Thread(target=pipe, args=(remote, client_sock, "r2c", _raw_stats), daemon=True)
    t1.start()
    t2.start()
    t1.join()
    t2.join()
    try:
        client_sock.close()
    except Exception:
        pass
    try:
        remote.close()
    except Exception:
        pass


# ---------------- 明文 HTTP 代理 ----------------
def handle_plain(conn, buf):
    header_blob, _, rest = buf.partition(b"\r\n\r\n")
    lines = header_blob.split(b"\r\n")
    first = lines[0].decode("utf-8", "replace")
    parts = first.split(" ")
    method = parts[0]
    target = parts[1]
    from urllib.parse import urlparse
    u = urlparse(target)
    host = u.hostname
    port = u.port or (443 if u.scheme == "https" else 80)
    # 非目标域名：静默转发，不记录日志
    _is_target = host_in_targets(host)
    if _is_target:
        log(f"  [plain] {method} {u.scheme}://{host}:{port}{u.path or '/'}")
    headers = {}
    for line in lines[1:]:
        if b":" in line:
            k, _, v = line.partition(b":")
            headers[k.decode("utf-8", "replace").strip().lower()] = v.decode("utf-8", "replace").strip()
    cl = int(headers.get("content-length", 0) or 0)
    body = rest
    while len(body) < cl:
        chunk = conn.recv(4096)
        if not chunk:
            break
        body += chunk
    body = body[:cl] if cl else body
    ctx = ssl.create_default_context() if u.scheme == "https" else None
    c = http.client.HTTPSConnection(host, port, context=ctx, timeout=30) if u.scheme == "https" else http.client.HTTPConnection(host, port, timeout=30)
    fwd = {k: v for k, v in headers.items() if k.lower() not in HOP_BY_HOP}
    # 非目标域名且配置了上游代理：经上游(通常是 VPN)转发，使外网明文 HTTP 也正常可用
    if (not _is_target) and UPSTREAM_PROXY and u.scheme == "http":
        _up = _parse_upstream(UPSTREAM_PROXY)
        if _up and _up[0] == "http":
            try:
                _uh, _uport = _split_host_port(_up[1], 8080)
                _c = http.client.HTTPConnection(_uh, _uport, timeout=30)
                # 向上游 HTTP 代理发送带绝对 URL 的请求
                _c.request(method, target, body=body if method.upper() != "GET" else None, headers=fwd)
                _resp = _c.getresponse()
                _resp_body = _resp.read()
                send_response(conn, _resp.status, _resp.reason, dict(_resp.getheaders()), _resp_body)
                _c.close()
                return
            except Exception:
                pass  # 回退到下面的直连逻辑
    try:
        c.request(method, u.path or "/", body=body if method.upper() != "GET" else None, headers=fwd)
        resp = c.getresponse()
        resp_body = resp.read()
        if _is_target:
            log(f"  [plain] <- {resp.status} {resp.reason} ({len(resp_body)} bytes) from {host}{u.path}")
        send_response(conn, resp.status, resp.reason, dict(resp.getheaders()), resp_body)
        # 仅目标域名记录到代理请求日志
        if _is_target:
            pl = get_proxy_logger()
            if pl:
                pl.log_request(method, host, u.path or "/", headers, body, resp.status, resp.reason, resp.getheaders(), resp_body)
    except Exception as e:
        if _is_target:
            log(f"  [plain] 错误: {type(e).__name__}: {e} (host={host}, path={u.path})")
        send_response(conn, 502, "Bad Gateway", {}, b"Bad Gateway")
    finally:
        c.close()

# ---------------- 客户端连接分发 ----------------
def handle_client(conn, addr):
    try:
        buf = conn.recv(4096)
        if not buf:
            return
        while b"\r\n\r\n" not in buf:
            more = conn.recv(4096)
            if not more:
                break
            buf += more
        head = buf.split(b"\r\n", 1)[0].decode("utf-8", "replace")
        method = head.split(" ")[0]
        if method == "CONNECT":
            target = head.split(" ")[1]
            host = target.rsplit(":", 1)[0]
            port = int(target.rsplit(":", 1)[1]) if ":" in target else 443
            is_target = host_in_targets(host)
            if not is_target:
                # 非目标域名：透明隧道转发（不解密、不记录日志），保证其他 App 正常上网
                tunnel_raw(conn, host, port)
                return
            matched = next((d for d in TARGET_DOMAINS if host == d or host.endswith("." + d)), None)
            log(f"CONNECT {host}:{port}  [TRAE/MITM] 匹配域名: {matched}")
            conn.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            # TRAE 域名：MITM 解密以捕获 JWT
            cpath, kpath = leaf_cert(host)
            ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            ctx.load_cert_chain(certfile=cpath, keyfile=kpath)
            tls = ctx.wrap_socket(conn, server_side=True)
            tunnel_https(tls, host, port)
        else:
            # 明文 HTTP 请求：全部转发（非目标域名的日志由 handle_plain 内部控制）
            handle_plain(conn, buf)
    except Exception as e:
        log(f"client err: {type(e).__name__}: {e}")
    finally:
        try:
            conn.close()
        except Exception:
            pass

# ---------------- 主入口 ----------------
def sync_account_devices():
    """把 checkin_accounts.json 中各账号的 device 字段刷新为当前算法生成的设备标识。

    用于升级历史记录中由旧算法生成的假占位符(device_id 全 '2' / session_id 全 '5')。
    启动时代理会调用一次；仅当字段确实变化时才写盘。
    """
    cfg = load_accounts()
    accounts = cfg.get("accounts", [])
    if not accounts:
        return
    changed = False
    for a in accounts:
        uid = a.get("UserID") or a.get("user_id")
        if not uid:
            continue
        dev = get_device_for(str(uid))
        if (a.get("device_id") != dev["device_id"]
                or a.get("session_id") != dev["session_id"]
                or a.get("market_user_id") != dev["market_user_id"]):
            a["device_id"] = dev["device_id"]
            a["session_id"] = dev["session_id"]
            a["market_user_id"] = dev["market_user_id"]
            changed = True
    if changed:
        save_accounts()
        log("  [sync] 已刷新 accounts.json 中设备标识字段(旧算法升级)")


def main():
    # --gen-ca：仅生成 CA 证书后退出（供桌面端证书安装流程调用）
    if "--gen-ca" in sys.argv:
        ensure_ca()
        log("CA 证书已生成，退出。")
        return 0

    # --capture-local：直接从 TRAE 本地 Cookies 解密提取 Cloud-IDE-JWT 写回 accounts.json
    # （代理方案在当前 TRAE 版本抓不到鉴权请求时的兜底；仅 Windows 有效）
    if "--capture-local" in sys.argv:
        n = capture_from_local()
        log(f"本地捕获结果: {n} 个账号")
        return 0

    ensure_ca()
    load_map()
    if AUTO_CAPTURE_JWT:
        load_accounts()  # 预热 accounts 缓存
        sync_account_devices()  # 升级历史假占位符设备标识
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind((LISTEN_HOST, LISTEN_PORT))
    srv.listen(128)
    log(f"代理已启动: {LISTEN_HOST}:{LISTEN_PORT}  (TRAE 多域 MITM 拦截 + JWT 自动捕获)")
    log("监听 TRAE 域名: " + ", ".join("*." + d for d in TARGET_DOMAINS))
    log("  → 命中上述域名的请求会在面板中以 [TRAE] 标记；JWT 捕获不限 host（兼容未列出的子域）")
    log("  → 未在监听域名列表中的请求将透明转发（不记录日志），不影响其他 App 正常上网")
    log(f"映射文件: {MAP_FILE}   日志: {LOG_FILE}   accounts: {ACCOUNTS_FILE}")
    log(f"代理请求日志: {PROXY_LOG_DIR} (100MB 滚动)")
    log(f"自动捕获 JWT 写回 accounts.json: {'开' if AUTO_CAPTURE_JWT else '关 (AUTO_CAPTURE_JWT=0)'}")
    if AUTO_CAPTURE_JWT:
        log("  → TRAE 中点签到时，新 JWT 会自动覆盖到 checkin_accounts.json（按 user_id 匹配，带 exp 防降级）")
    log("请把 CA 证书 certs/ca.cer 安装到 Windows 受信任根证书颁发机构(管理员)。")
    try:
        while True:
            try:
                conn, addr = srv.accept()
            except Exception as e:
                # 单条 accept 出错不应让整个代理进程退出（否则系统代理仍指向死端口）。
                # 记录后短暂退避再重试，保持服务可用。
                log(f"[accept] 异常(已忽略并重试): {type(e).__name__}: {e}")
                time.sleep(0.1)
                continue
            t = threading.Thread(target=handle_client, args=(conn, addr), daemon=True)
            t.start()
    except KeyboardInterrupt:
        log("代理停止")

if __name__ == "__main__":
    sys.exit(main() or 0)
