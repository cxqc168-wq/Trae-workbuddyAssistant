#!/usr/bin/env python3
"""
Trae Work Assistant API 服务端到端测试

测试项目:
  1. /health          — 健康检查（含账号池状态）
  2. /status          — 完整状态（含账号明细）
  3. /v1/models       — 模型列表
  4. /v1/chat/completions (非流式) — 单轮对话
  6. /v1/chat/completions (流式)   — SSE 流式对话
  7. 鉴权测试          — 无 API Key / 错误 Key
  8. 错误处理          — 无效模型

用法:
    python test_api_server.py                    # 使用默认配置
    python test_api_server.py --port 7864        # 指定端口
    python test_api_server.py --key sk-xxx       # 指定 API Key
    python test_api_server.py --model glm-5.2    # 指定测试模型
"""
import json
import sys
import time
import argparse
import http.client
from urllib.parse import urlparse


class ApiTester:
    def __init__(self, host, port, api_key, model):
        self.host = host
        self.port = port
        self.api_key = api_key
        self.model = model
        self.passed = 0
        self.failed = 0
        self.skipped = 0

    def _request(self, method, path, body=None, headers=None, timeout=10):
        """发送 HTTP 请求，返回 (status, headers, body_bytes)"""
        conn = http.client.HTTPConnection(self.host, self.port, timeout=timeout)
        try:
            h = {"Host": f"{self.host}:{self.port}"}
            if self.api_key:
                h["Authorization"] = f"Bearer {self.api_key}"
            if headers:
                h.update(headers)
            if body is not None:
                h["Content-Type"] = "application/json"
            conn.request(method, path, body=body, headers=h)
            resp = conn.getresponse()
            data = resp.read()
            return resp.status, dict(resp.getheaders()), data
        finally:
            conn.close()

    def _request_stream(self, path, body, timeout=60):
        """发送流式请求，返回 (status, chunks[])"""
        conn = http.client.HTTPConnection(self.host, self.port, timeout=timeout)
        try:
            headers = {
                "Authorization": f"Bearer {self.api_key}",
                "Content-Type": "application/json",
            }
            conn.request("POST", path, body=body, headers=headers)
            resp = conn.getresponse()
            chunks = []
            if resp.status == 200:
                while True:
                    line = resp.readline()
                    if not line:
                        break
                    chunks.append(line.decode("utf-8", "replace").strip())
            else:
                data = resp.read()
                chunks.append(data.decode("utf-8", "replace"))
            return resp.status, chunks
        finally:
            conn.close()

    def check(self, name, condition, detail=""):
        if condition:
            self.passed += 1
            print(f"  ✓ {name}")
        else:
            self.failed += 1
            print(f"  ✗ {name}")
            if detail:
                print(f"    {detail}")

    def skip(self, name, reason=""):
        self.skipped += 1
        print(f"  - {name} (跳过: {reason})")

    # ── 测试用例 ──────────────────────────────────

    def test_health(self):
        """1. 健康检查（含账号池状态）"""
        print("\n[1] /health — 健康检查")
        try:
            status, _, body = self._request("GET", "/health")
            self.check("返回 200", status == 200, f"实际 status={status}")
            data = json.loads(body)
            self.check("包含 status 字段", "status" in data, f"keys={list(data.keys())}")
            self.check("包含 pool 字段", "pool" in data, f"keys={list(data.keys())}")
            pool = data.get("pool", {})
            print(f"    账号池: total={pool.get('total_accounts', '?')} "
                  f"available={pool.get('available', '?')} "
                  f"cooling={pool.get('cooling', '?')} "
                  f"credits={pool.get('total_credits', '?')}")
        except Exception as e:
            self.check("/health 请求成功", False, str(e))

    def test_status(self):
        """3. 完整状态"""
        print("\n[3] /status — 完整状态（含账号明细）")
        try:
            status, _, body = self._request("GET", "/status")
            self.check("返回 200", status == 200, f"实际 status={status}")
            data = json.loads(body)
            accounts = data.get("accounts", [])
            self.check("包含 accounts 列表", isinstance(accounts, list), f"type={type(accounts)}")
            print(f"    账号数: {len(accounts)}")
            for a in accounts[:5]:
                print(f"    {a.get('name', '?')} status={a.get('status', '?')} "
                      f"credits={a.get('credits', '?')} err={a.get('err_count', 0)}")
            if len(accounts) > 5:
                print(f"    ... 还有 {len(accounts) - 5} 个账号")
        except Exception as e:
            self.check("/status 请求成功", False, str(e))

    def test_models(self):
        """4. 模型列表"""
        print("\n[4] /v1/models — 模型列表")
        try:
            status, _, body = self._request("GET", "/v1/models")
            self.check("返回 200", status == 200, f"实际 status={status}")
            data = json.loads(body)
            self.check("object=list", data.get("object") == "list", f"object={data.get('object')}")
            models = data.get("data", [])
            self.check("模型列表非空", len(models) > 0, "models 为空")
            model_ids = [m.get("id", "") for m in models]
            print(f"    可用模型 ({len(model_ids)} 个): {', '.join(model_ids[:8])}")
            if len(model_ids) > 8:
                print(f"    ... 还有 {len(model_ids) - 8} 个")
        except Exception as e:
            self.check("/v1/models 请求成功", False, str(e))

    def test_chat_non_stream(self):
        """5. 非流式对话"""
        print("\n[5] /v1/chat/completions (非流式) — 单轮对话")
        body = json.dumps({
            "model": self.model,
            "messages": [{"role": "user", "content": "reply hello, just two words"}],
            "stream": False,
            "max_tokens": 50,
        }).encode()
        try:
            status, _, resp_body = self._request("POST", "/v1/chat/completions", body=body, timeout=120)
            self.check("返回 200", status == 200, f"实际 status={status}")

            if status == 200:
                data = json.loads(resp_body)
                self.check("object=chat.completion", data.get("object") == "chat.completion",
                           f"object={data.get('object')}")
                choices = data.get("choices", [])
                self.check("choices 非空", len(choices) > 0, "choices 为空")
                if choices:
                    msg = choices[0].get("message", {})
                    content = msg.get("content", "")
                    self.check("content 非空", len(content) > 0, "content 为空")
                    print(f"    模型回复: {content[:100]}")
                    finish = choices[0].get("finish_reason", "")
                    print(f"    finish_reason: {finish}")
                if "usage" in data:
                    u = data["usage"]
                    print(f"    token: prompt={u.get('prompt_tokens','?')} "
                          f"completion={u.get('completion_tokens','?')}")
            else:
                text = resp_body.decode("utf-8", "replace")[:300]
                print(f"    响应: {text}")
        except Exception as e:
            self.check("非流式请求成功", False, str(e))

    def test_chat_stream(self):
        """6. 流式对话"""
        print("\n[6] /v1/chat/completions (流式) — SSE 流式对话")
        body = json.dumps({
            "model": self.model,
            "messages": [{"role": "user", "content": "reply hello, just two words"}],
            "stream": True,
            "max_tokens": 50,
        }).encode()
        try:
            status, chunks = self._request_stream("/v1/chat/completions", body, timeout=120)
            self.check("返回 200", status == 200, f"实际 status={status}")

            if status == 200:
                data_chunks = [c for c in chunks if c.startswith("data: ") and "DONE" not in c]
                self.check("收到 SSE data 事件", len(data_chunks) > 0, "无 data 事件")

                content_parts = []
                finish_reason = None
                has_usage = False

                for chunk in data_chunks:
                    try:
                        obj = json.loads(chunk[6:])
                        if obj.get("object") == "chat.completion.chunk":
                            choices = obj.get("choices", [])
                            if choices:
                                delta = choices[0].get("delta", {})
                                if "content" in delta:
                                    content_parts.append(delta["content"])
                                if choices[0].get("finish_reason"):
                                    finish_reason = choices[0]["finish_reason"]
                            if "usage" in obj:
                                has_usage = True
                    except json.JSONDecodeError:
                        pass

                content = "".join(content_parts)
                self.check("拼接 content 非空", len(content) > 0, "content 为空")
                self.check("有 finish_reason", finish_reason is not None, "无 finish_reason")
                print(f"    流式回复: {content[:100]}")
                print(f"    finish_reason: {finish_reason}")
                print(f"    data 事件数: {len(data_chunks)}")
                if has_usage:
                    print(f"    含 usage 信息: 是")
        except Exception as e:
            self.check("流式请求成功", False, str(e))

    def test_auth(self):
        """7. 鉴权测试"""
        print("\n[7] 鉴权测试")
        # 无 API Key
        try:
            conn = http.client.HTTPConnection(self.host, self.port, timeout=5)
            conn.request("GET", "/v1/models")
            resp = conn.getresponse()
            body = resp.read()
            conn.close()
            self.check("无 Key 返回 401", resp.status == 401,
                       f"实际 status={resp.status}")
        except Exception as e:
            self.check("无 Key 请求", False, str(e))

        # 错误 API Key
        try:
            conn = http.client.HTTPConnection(self.host, self.port, timeout=5)
            conn.request("GET", "/v1/models", headers={"Authorization": "Bearer wrong-key"})
            resp = conn.getresponse()
            body = resp.read()
            conn.close()
            self.check("错误 Key 返回 401", resp.status == 401,
                       f"实际 status={resp.status}")
        except Exception as e:
            self.check("错误 Key 请求", False, str(e))

    def test_error_handling(self):
        """8. 错误处理 — 无效模型"""
        print("\n[8] 错误处理 — 无效模型名")
        body = json.dumps({
            "model": "invalid-model-xxx",
            "messages": [{"role": "user", "content": "test"}],
            "stream": False,
            "max_tokens": 10,
        }).encode()
        try:
            status, _, resp_body = self._request("POST", "/v1/chat/completions", body=body, timeout=120)
            # 无效模型会回退到默认模型，所以应该返回 200 或 503
            if status == 200:
                data = json.loads(resp_body)
                self.check("无效模型回退到默认", True, "回退成功")
                print(f"    回退后正常返回（status=200）")
            elif status == 503:
                self.check("无可用账号时返回 503", True)
                print(f"    返回 503（可能无可用账号）")
            else:
                self.check(f"返回合理状态码 (实际 {status})", status in (200, 400, 404, 503),
                           f"意外 status={status}")
                text = resp_body.decode("utf-8", "replace")[:200]
                print(f"    响应: {text}")
        except Exception as e:
            self.check("错误处理请求", False, str(e))

    # ── 主入口 ──────────────────────────────────

    def run_all(self):
        print("=" * 60)
        print(f"Trae Work Assistant API 端到端测试")
        print(f"地址: http://{self.host}:{self.port}")
        print(f"模型: {self.model}")
        print(f"Key:  {self.api_key[:12]}..." if self.api_key else "Key:  (无)")
        print("=" * 60)

        # 先检查服务是否在线
        try:
            conn = http.client.HTTPConnection(self.host, self.port, timeout=3)
            conn.request("GET", "/health")
            resp = conn.getresponse()
            conn.close()
        except Exception:
            print(f"\n✗ API 服务未运行！请在应用中启动 API 服务（端口 {self.port}）")
            sys.exit(1)

        t0 = time.time()

        self.test_health()
        self.test_status()
        self.test_models()
        self.test_chat_non_stream()
        self.test_chat_stream()
        self.test_auth()
        self.test_error_handling()

        elapsed = time.time() - t0
        print("\n" + "=" * 60)
        print(f"测试完成: {self.passed} 通过, {self.failed} 失败, {self.skipped} 跳过 "
              f"({elapsed:.1f}s)")
        print("=" * 60)
        return 0 if self.failed == 0 else 1


def main():
    parser = argparse.ArgumentParser(description="Trae Work Assistant API 端到端测试")
    parser.add_argument("--host", default="127.0.0.1", help="API 服务地址")
    parser.add_argument("--port", type=int, default=7864, help="API 服务端口")
    parser.add_argument("--key", default="sk-72a12ee8-b462-4b03-837f-de0646fb419f-64aad",
                        help="API Key")
    parser.add_argument("--model", default="glm-5.2", help="测试模型")
    args = parser.parse_args()

    tester = ApiTester(args.host, args.port, args.key, args.model)
    sys.exit(tester.run_all())


if __name__ == "__main__":
    main()
