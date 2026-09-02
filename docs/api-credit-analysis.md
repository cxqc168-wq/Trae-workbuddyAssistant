# Trae API 积分消耗与接口分析文档

> **文档版本**: 2026-08-14
> **分析来源**: MITM 代理抓包日志 + 项目代码实现 + 明文 JSON 测试验证
> **涉及项目**: trae-work-assistant (TraeWorkAssistant)

---

## 目录

1. [积分体系概述](#1-积分体系概述)
2. [接口地址与路由](#2-接口地址与路由)
3. [请求体加密机制](#3-请求体加密机制)
4. [请求头分析](#4-请求头分析)
5. [llm_utils_chat 接口分析（IDE 积分）](#5-llm_utils_chat-接口分析ide-积分)
6. [create_agent_task 接口分析（Work 积分）](#6-create_agent_task-接口分析work-积分)
7. [SSE 响应事件对比](#7-sse-响应事件对比)
8. [明文 JSON 测试验证](#8-明文-json-测试验证)
9. [项目实现详情](#9-项目实现详情)
10. [账号池与健康检查逻辑](#10-账号池与健康检查逻辑)
11. [错误分类与冷却策略](#11-错误分类与冷却策略)
12. [模型名映射表](#12-模型名映射表)

---

## 1. 积分体系概述

Trae 平台存在两种独立的积分体系，分别对应不同的 API 接口：

| 属性 | IDE 积分 | Work 积分 |
|------|----------|-----------|
| **product_id** | 208 | 209 |
| **消耗接口** | `/api/agent/v3/llm_utils_chat` | `/api/agent/v3/create_agent_task` |
| **使用场景** | Trae Code (IDE) 内的 AI 对话 | Trae Work (SOLO Work) 的 Agent 任务 |
| **获取方式** | 订阅 IDE 套餐 | 订阅 Work 套餐 / 每日签到 / 购买 |
| **积分查询** | `ide_user_pay_status` 接口 | `get_session_usage` 接口 |

### 核心问题

项目最初使用 `llm_utils_chat` 接口，该接口**仅消耗 IDE 积分**（product_id 208），无法使用 Work 积分。当账号的 IDE 积分耗尽但 Work 积分充足时，账号被错误标记为不可用，导致"no healthy account"错误。

**解决方案**：切换到 `create_agent_task` 接口，该接口消耗 Work 积分（product_id 209），与每日签到获得的积分类型一致。

---

## 2. 接口地址与路由

### 2.1 域名架构

```
真实请求主机:  api5-normal.mchost.guru  (实际 LLM 网关)
Referer 域名:  trae-api-cn.mchost.guru  (页面/Referer 用途)
```

Trae 客户端的真实请求发往 `api5-normal.mchost.guru`，而 `trae-api-cn.mchost.guru` 仅作为 Referer 头和页面域名使用。项目中两个常量分别对应：

```rust
// src-tauri/src/api_server/mod.rs
pub const AGENT_HOST: &str = "https://api5-normal.mchost.guru";
pub const REFERER_BASE: &str = "https://trae-api-cn.mchost.guru";
```

### 2.2 接口端点

| 接口 | 端点路径 | 完整 URL |
|------|----------|----------|
| llm_utils_chat (旧) | `/api/agent/v3/llm_utils_chat` | `https://api5-normal.mchost.guru/api/agent/v3/llm_utils_chat` |
| create_agent_task (新) | `/api/agent/v3/create_agent_task` | `https://api5-normal.mchost.guru/api/agent/v3/create_agent_task` |

### 2.3 本项目 API 服务路由

本项目对外暴露 OpenAI 兼容的 API 接口：

| 路由 | 方法 | 功能 |
|------|------|------|
| `/v1/chat/completions` | POST | 对话补全（流式/非流式） |
| `/v1/models` | GET | 模型列表 |
| `/health` | GET | 健康检查 |
| `/status` | GET | 详细状态（含账号池） |

---

## 3. 请求体加密机制

### 3.1 TTNet / aha 加密层

Trae 客户端在发送 `create_agent_task` 请求时，请求体经过 **TTNet/aha 加密层**加密。从 MITM 抓包日志可见：

```
Content-Length: 105272
Content-Type: application/json
x-bridge-transport: aha
```

请求体内容为不可读的二进制数据（Base64 编码的加密载荷），而非明文 JSON：

```
d+ljQ2d0/zffEKpBSdS+AghbUSXVfT8VjnCWcAFGenKtH1KflQp3unVc8Wc6xAKgF3AyogHPlV13...
(105272 bytes 加密数据)
```

### 3.2 加密相关请求头

| 请求头 | 值 | 说明 |
|--------|-----|------|
| `x-bridge-transport` | `aha` | 标识使用 aha 加密传输 |
| `x-helios` | Base64 编码 | Helios 安全令牌（客户端原生代码生成） |
| `x-medusa` | Base64 编码 | Medusa 安全令牌（客户端原生代码生成） |
| `x-neptune` | `-11\|50:51:59:00:09` | Neptune 时间戳签名 |
| `x-request-pin` | `87b6ad880149ece7` | 请求 PIN 码 |

### 3.3 明文 JSON 可行性验证

通过测试脚本 `test_create_agent_task.py` 验证，服务器**同时接受明文 JSON 请求体**。这意味着：

- **带 `x-bridge-transport: aha` 头**：服务器先解密请求体再处理
- **不带该头或发送明文 JSON**：服务器直接解析 JSON

项目中选择发送明文 JSON（简化实现，无需逆向加密算法），仅保留必要的安全头（`x-helios`、`x-medusa`、`x-neptune` 通过透传方式处理）。

> **风险提示**: `x-helios`/`x-medusa`/`x-neptune` 令牌由 Trae 客户端原生代码生成，本项目无法复制。目前省略这些头仍可正常调用，但未来可能被服务端检测。

---

## 4. 请求头分析

### 4.1 完整请求头列表

以下是从 Trae Work 客户端实际请求中抓取的完整请求头（共 30+ 个）：

#### 认证与身份头

| 请求头 | 示例值 | 说明 |
|--------|--------|------|
| `x-ide-token` | `eyJhbGciOiJSUzI1NiIs...` | JWT 认证令牌（Cloud-IDE-JWT 格式） |
| `x-app-id` | `6eefa01c-1036-4c7e-9ca5-d891f63bfcd8` | 应用 ID（固定值） |
| `x-device-id` | `199439841787403` | 设备 ID（每账号唯一） |
| `x-machine-id` | `b04f40320d4f2d71...` (64 hex) | 机器 ID（由 uid 派生） |

#### 版本与平台头

| 请求头 | 示例值 | 说明 |
|--------|--------|------|
| `x-ide-version` | `0.1.50` | IDE 版本号 |
| `x-ide-version-code` | `20260811` | IDE 版本代码 |
| `x-ide-version-type` | `stable` | 版本类型 |
| `x-app-version` | `default` | 应用版本 |
| `x-app-version-code` | `20260811` | 应用版本代码 |
| `app-version` | `0.1.50` | 应用版本（重复） |
| `x-device-type` | `windows` | 设备类型 |
| `x-device-brand` | `CREFG-XX` | 设备品牌 |
| `x-device-cpu` | `Intel` | CPU 类型 |
| `x-os-version` | `Windows 11 Home China` | OS 版本 |
| `package-type` | `stable_cn` | 包类型（中国区稳定版） |
| `request-traffic-type` | `prod` | 流量类型（生产环境） |

#### SDK 与追踪头

| 请求头 | 示例值 | 说明 |
|--------|--------|------|
| `x-lgw-req-sdk-type` | `3` | SDK 类型（3 = SOLO Work） |
| `x-lscbd-aid` | `787976` | LSCBD 应用 ID |
| `x-lscbd-platform` | `windows` | LSCBD 平台 |
| `x-ss-dp` | `787976` | SS-DP 标识 |
| `x-custom-trace-id` | `83746ed87512d981...` | 自定义追踪 ID |
| `x-flow-traceparent` | `04-83746ed8...-fb46e995...-01` | 流追踪父 ID |
| `x-tt-trace-id` | `00-002a565b0cb5...-002a565b0cb5...-01` | TT 追踪 ID |
| `x-request-id` | `req_1847badb-cdeb-4108-...` | 请求 ID |
| `x-trae-request-id` | `aa5571bd-bacd-4616-...` | Trae 请求 ID |
| `x-requested-at` | `1786709169` | 请求时间戳 |

#### 安全头（客户端原生生成）

| 请求头 | 说明 |
|--------|------|
| `x-helios` | Helios 安全令牌 |
| `x-medusa` | Medusa 安全令牌 |
| `x-neptune` | Neptune 签名 |
| `x-request-pin` | 请求 PIN |

#### 其他头

| 请求头 | 说明 |
|--------|------|
| `content-type` | `application/json` |
| `accept` | `*/*` |
| `accept-encoding` | `gzip, deflate, br, zstd` |
| `user-agent` | `TraeClient/TTNet` |
| `referer` | `https://trae-api-cn.mchost.guru/api/agent/v3/create_agent_task` |
| `x-ahanet-timeout` | `86400` |

### 4.2 项目中使用的请求头

项目代码（`routes.rs` 中的 `make_upstream_request`）设置了以下 28 个请求头：

```rust
.set("content-type", "application/json")
.set("accept", "*/*")
.set("accept-encoding", "gzip, deflate, br, zstd")
.set("user-agent", "TraeClient/TTNet")
.set("x-ide-token", jwt)                        // JWT 令牌
.set("x-app-id", APP_ID)                        // 应用 ID
.set("x-app-version", "default")
.set("x-app-version-code", IDE_VERSION_CODE)
.set("x-ide-version", IDE_VERSION)
.set("x-ide-version-code", IDE_VERSION_CODE)
.set("x-ide-version-type", "stable")
.set("x-device-type", "windows")
.set("x-device-brand", "CREFG-XX")
.set("x-device-cpu", "Intel")
.set("x-device-id", device_id)                  // 账号专属设备 ID
.set("x-machine-id", machine_id)                // 账号专属机器 ID
.set("x-os-version", "Windows 11 Home China")
.set("request-traffic-type", "prod")
.set("package-type", "stable_cn")
.set("x-bridge-transport", "aha")               // 加密传输标识
.set("x-lgw-req-sdk-type", "3")                 // SDK 类型
.set("x-lscbd-aid", "787976")
.set("x-lscbd-platform", "windows")
.set("x-ss-dp", "787976")
.set("app-version", IDE_VERSION)
.set("x-custom-trace-id", &trace_id[..16])
.set("x-flow-traceparent", ...)
.set("x-tt-trace-id", &trace_id)
.set("x-request-id", &request_id)
.set("referer", &referer)
```

**未包含的安全头**：`x-helios`、`x-medusa`、`x-neptune`、`x-request-pin`、`x-ahanet-timeout`、`x-requested-at`、`x-trae-request-id`（由 Trae 原生代码生成，无法复制）。

---

## 5. llm_utils_chat 接口分析（IDE 积分）

### 5.1 基本信息

| 属性 | 值 |
|------|-----|
| **端点** | `/api/agent/v3/llm_utils_chat` |
| **方法** | POST |
| **消耗积分** | IDE 积分（product_id 208） |
| **响应格式** | SSE (text/event-stream) |
| **请求体格式** | JSON（明文或 aha 加密） |

### 5.2 请求体结构

```json
{
  "messages": [
    {"role": "user", "content": [{"type": "text", "text": "你好"}]}
  ],
  "model": "DeepSeek-V4-Flash",
  "config_name": "DeepSeek-V4-Flash",
  "stream": true,
  "function": "solo_work_lite",
  "max_tokens": 4096,
  "conversation_id": "uuid",
  "user_id": "uid",
  "session_id": "uuid",
  "device_id": "199439841787403",
  "machine_id": "b04f40320d4f...",
  "project_id": "uuid",
  "workspace_id": "e04cdd",
  "prompt_max_tokens": 168000,
  "mode": "FunctionCall",
  "ide_version": "0.1.50",
  "ide_version_code": "20260811",
  "app_id": "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8",
  "package_type": "stable_cn"
}
```

### 5.3 SSE 响应事件

| 事件类型 | 说明 | 关键字段 |
|----------|------|----------|
| `event:output` | 模型输出内容 | `response`(正文), `reasoning_content`(推理), `tool_calls`(工具调用) |
| `event:token_usage` | Token 用量 | `prompt_tokens`, `completion_tokens`, `total_tokens` |
| `event:done` | 结束标记 | `finish_reason` |
| `event:error` | 错误事件 | `code`, `message` |

### 5.4 局限性

- 仅消耗 IDE 积分，Work 积分无法使用
- 每日签到获得的积分是 Work 积分（product_id 209），无法用于此接口
- IDE 积分耗尽后账号变为不可用，即使 Work 积分充足

---

## 6. create_agent_task 接口分析（Work 积分）

### 6.1 基本信息

| 属性 | 值 |
|------|-----|
| **端点** | `/api/agent/v3/create_agent_task` |
| **方法** | POST |
| **消耗积分** | Work 积分（product_id 209） |
| **响应格式** | SSE (text/event-stream) |
| **请求体格式** | JSON（明文或 aha 加密） |
| **请求体大小** | Trae 客户端加密后约 105KB；明文 JSON 约 2-5KB |

### 6.2 请求体结构（明文 JSON）

从 MITM 抓包和测试脚本验证得出的完整请求体结构：

```json
{
  "messages": [
    {"role": "user", "content": [{"type": "text", "text": "你好"}]}
  ],
  "user_input": {
    "id": "uuid-字符串",
    "text": "你好"
  },
  "model": "DeepSeek-V4-Flash",
  "config_name": "DeepSeek-V4-Flash",
  "model_name": "deepseek_v4_flash__dev",
  "stream": true,
  "function": "solo_work_lite",
  "agent_type": "solo_work_lite",
  "agent_id": "solo_work_lite",
  "max_tokens": 4096,
  "conversation_id": "uuid",
  "user_id": "uid",
  "session_id": "uuid",
  "device_id": "199439841787403",
  "machine_id": "b04f40320d4f...",
  "project_id": "uuid",
  "workspace_id": "e04cdd",
  "prompt_max_tokens": 168000,
  "mode": "FunctionCall",
  "ide_version": "0.1.50",
  "ide_version_code": "20260811",
  "app_id": "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8",
  "package_type": "stable_cn",
  "summary_config": {"enabled": false},
  "chat_memory_config": {"enabled": false},
  "is_new_session": true
}
```

### 6.3 与 llm_utils_chat 的请求体差异

| 字段 | llm_utils_chat | create_agent_task | 说明 |
|------|----------------|-------------------|------|
| `user_input` | 不需要 | **必需**（对象格式） | 包含 `id` 和 `text` 字段 |
| `model_name` | 不需要 | **必需** | 内部模型标识（如 `deepseek_v4_flash__dev`） |
| `agent_type` | 不需要 | **推荐** | 值与 `function` 相同 |
| `agent_id` | 不需要 | **推荐** | 值与 `function` 相同 |
| `summary_config` | 不需要 | **推荐** | 禁用摘要功能避免错误 |
| `chat_memory_config` | 不需要 | **推荐** | 禁用记忆功能避免历史依赖 |
| `is_new_session` | 不需要 | **必需** | 避免触发 "missing history" 错误 |

### 6.4 必填参数验证过程

通过逐步添加参数测试，得出以下必填字段及其错误信息：

| 缺失字段 | 错误信息 | 修复方式 |
|----------|----------|----------|
| `user_input`（字符串格式） | `json: cannot unmarshal string into Go struct field CreateAgentTaskRequest.user_input of type ideagent.UserInput` | 改为对象格式 `{"id": "...", "text": "..."}` |
| `user_input.id` | `binding: expr_path=user_input.id, cause=missing required parameter` | 添加 UUID 格式的 `id` 字段 |
| `summary_config` | `failed to get summary config: failed to get summary template data` | 添加 `{"enabled": false}` |
| `is_new_session` | `4000105 - missing history count exceeded for session <session_id>` | 添加 `true` 并生成新的 `conversation_id` 和 `session_id` |

### 6.5 响应头示例

```
HTTP/1.1 200 OK
Server: volc-dcdn
Content-Type: text/event-stream
Transfer-Encoding: chunked
Connection: keep-alive
Cache-Control: no-cache
Strict-Transport-Security: max-age=31536000; includeSubDomains
X-Tt-Logid: 2026081420062102CBFCEEE9EE0243B56C
x-tt-trace-tag: id=5
x-request-ip: 116.147.154.117
```

---

## 7. SSE 响应事件对比

### 7.1 事件类型对比表

| 事件类型 | llm_utils_chat | create_agent_task | 项目处理方式 |
|----------|----------------|-------------------|-------------|
| `event:output` | ✅ 模型输出 | ❌ 不存在 | 提取 `response` → content |
| `event:thought` | ❌ 不存在 | ✅ 模型输出 | 提取 `thought` → content |
| `event:token_usage` | ✅ | ✅ | 转换为 OpenAI usage 格式 |
| `event:done` | ✅ 结束标记 | ❌ 不存在 | 设置 finish_reason |
| `event:turn_completion` | ❌ 不存在 | ✅ 结束标记 | 设置 finish_reason="stop" |
| `event:error` | ✅ | ✅ | 转换为 OpenAI error 格式 |
| `event:task_created` | ❌ | ✅ | 忽略 |
| `event:model_config` | ❌ | ✅ | 忽略 |
| `event:tool_cache_data` | ❌ | ✅ | 忽略 |
| `event:agent_status` | ❌ | ✅ | 忽略 |
| `event:chat_memory_trigger` | ❌ | ✅ | 忽略 |
| `event:history` | ❌ | ✅ | 忽略 |
| `event:metadata` | ❌ | ✅ | 忽略 |
| `event:timing_cost` | ❌ | ✅ | 忽略 |
| `event:required_context` | ❌ | ✅ | 忽略 |

### 7.2 create_agent_task 完整 SSE 事件流示例

以下是一次完整的 `create_agent_task` SSE 响应（用户发送 "hi"）：

```
id:1
event:task_created
data:{"task_id":"9f4cede7-fbfb-41d9-9362-a79117e8560d","agent_run_id":"8890670c-c42e-5384-a459-6525b7a45b18"}

id:2
event:model_config
data:{"config_name":"DeepSeek-V4-Flash","model_name":"deepseek_v4_flash__dev","extra_config":{...},"max_turn":500,"prompt_max_tokens":168000,"max_tokens":32000}

id:3
event:tool_cache_data
data:{"agent_id":"solo_work_lite","groups":[{"group_name":"integrated_code_mode","tools":[...]}]}

id:4
event:agent_status
data:{"agents":[{"agent_run_id":"8890670c-...","status":"running","run_mode":"foreground"}]}

id:5
event:chat_memory_trigger
data:{"agent_type":"solo_work_lite","chat_memory_scene":"extract_topics_and_update_memory"}

id:6
event:history
data:{"history_data":{"messages":"{\"raw_messages\":[...]}",...}}

id:7
event:metadata
data:{"model":"","session_id":"6a7f04b0a379dd8fd784edc9",...}

id:8
event:timing_cost
data:{"config_name":"DeepSeek-V4-Flash","gateway_preprocess_timing":132,...,"first_sse_event_time":1774}

id:9
event:thought
data:{"reasoning_content":"The","thought":""}

id:10
event:thought
data:{"reasoning_content":" user said \"hi\" which is English. Let me respond in English with a simple greeting.","thought":""}

id:11
event:thought
data:{"reasoning_content":"","thought":"Hi! How can I help you today?"}

id:12
event:token_usage
data:{"prompt_tokens":23561,"completion_tokens":31,"total_tokens":23592,"reasoning_tokens":20}

id:13
event:history
data:{"history_data":{"messages":"{\"raw_messages\":[{\"role\":\"assistant\",...}]}",...}}

id:14
event:required_context
data:{"contexts":["pending_message"]}

id:15
event:chat_memory_trigger
data:{"chat_memory_scene":"summarize_message"}

id:16
event:agent_status
data:{"agents":[{"agent_run_id":"8890670c-...","status":"completed","run_mode":"foreground"}]}

id:17
event:turn_completion
data:{"task_completion":true}
```

### 7.3 thought 事件字段说明

`event:thought` 事件是 `create_agent_task` 的核心内容输出事件：

| 字段 | 类型 | 说明 |
|------|------|------|
| `thought` | string | **模型回复正文**（对应 OpenAI 的 `content`） |
| `reasoning_content` | string | **推理过程**（CoT 链式思考，对应 `reasoning_content`） |
| `agent_type` | string | Agent 类型（如 `solo_work_lite`） |
| `agent_id` | string | Agent ID |
| `agent_name` | string | Agent 名称（如 `SOLO Work Lite`） |
| `agent_run_id` | string | 本次运行的唯一 ID |
| `first_data` | bool | 是否为首条数据 |
| `tool_calls` | array | 工具调用（如有） |

**流式输出模式**：
- 推理内容先于正文输出（id:9, id:10 为 `reasoning_content`，id:11 为 `thought`）
- 每个事件只包含增量内容（非全量）

### 7.4 token_usage 事件字段

```json
{
  "name": "",
  "prompt_tokens": 23561,
  "completion_tokens": 31,
  "total_tokens": 23592,
  "cache_creation_input_tokens": 0,
  "cache_read_input_tokens": 3072,
  "reasoning_tokens": 20,
  "cluster": "normal_context"
}
```

---

## 8. 明文 JSON 测试验证

### 8.1 测试脚本

测试脚本 `test_create_agent_task.py` 用于验证 `create_agent_task` 接口是否接受明文 JSON 请求（不经过 TTNet/aha 加密）。

### 8.2 测试过程与结果

| 测试阶段 | 参数变更 | 结果 |
|----------|----------|------|
| 初始测试 | 基础参数，`user_input` 为字符串 | 400 错误：`cannot unmarshal string into Go struct field` |
| 修复 user_input | 改为对象 `{"text": "..."}` | 400 错误：`missing required parameter user_input.id` |
| 添加 id | `user_input: {"id": uuid, "text": "..."}` | 400 错误：`failed to get summary config` |
| 添加 summary_config | `"summary_config": {"enabled": false}` | 400 错误：`missing history count exceeded` |
| 添加 is_new_session | `"is_new_session": true` + 新 session_id | **200 OK** ✅ |

### 8.3 结论

1. **服务器接受明文 JSON**：无需 TTNet/aha 加密即可调用
2. **必填参数**：`user_input`(对象)、`user_input.id`、`is_new_session`
3. **推荐参数**：`summary_config`(disabled)、`chat_memory_config`(disabled)
4. **每次请求需新会话**：`conversation_id`、`session_id`、`project_id` 每次生成新值

---

## 9. 项目实现详情

### 9.1 文件结构

```
src-tauri/src/api_server/
├── mod.rs       # 常量定义、错误分类、Agent 构建
├── payload.rs   # OpenAI → create_agent_task 请求体转换
├── routes.rs    # 路由处理、上游请求、账号轮换
├── sse.rs       # SSE 事件解析与 OpenAI 格式转换
├── pool.rs      # 账号池管理、健康检查
├── auth.rs      # API Key 鉴权
├── api_logger.rs # 请求日志记录
└── server.rs    # axum 服务器启停
```

### 9.2 请求处理流程

```
客户端 (OpenAI 格式)
    │
    ▼
chat_completions()          [routes.rs]
    │ 解析 model, stream
    │ 传递原始 body_vec
    ▼
stream_chat() / aggregate_chat()  [routes.rs]
    │ 循环最多 MAX_ROTATE(3) 次
    │
    ├─► pool.pick_excluding(&tried)  [pool.rs]
    │       选择 healthy 账号（跳过已试、冷却、零积分）
    │
    ├─► payload::prepare_create_task_body()  [payload.rs]
    │       OpenAI 格式 → create_agent_task 格式
    │       注入 uid, device_id, machine_id
    │
    ├─► make_upstream_request()  [routes.rs]
    │       构建 28 个请求头
    │       POST → api5-normal.mchost.guru
    │       返回 SSE 流 Reader
    │
    ├─► sse::stream_convert() / sse::aggregate()  [sse.rs]
    │       SOLO SSE → OpenAI SSE chunks
    │       thought → content, turn_completion → done
    │
    └─► 成功 → return / 失败 → note_error + continue
```

### 9.3 请求体转换（payload.rs）

`prepare_create_task_body` 函数负责将 OpenAI 格式请求体转换为 `create_agent_task` 格式：

```rust
pub fn prepare_create_task_body(
    src: &[u8],        // OpenAI 原始请求体
    default_model: &str,
    uid: &str,         // 账号 UID
    device_id: &str,   // 账号设备 ID
    machine_id: &str,  // 账号机器 ID
) -> Vec<u8>
```

**转换步骤**：

1. **提取用户消息文本**：从 `messages` 数组中找到最后一条 `role=user` 的消息，提取文本作为 `user_input.text`
2. **规范化 messages**：将 `content` 字符串转为数组格式 `[{type: text, text: ...}]`
3. **转换 tool_calls**：将 OpenAI 的 `function` 字段名转为 SOLO 的 `function_call`
4. **规范化 tool_choice 和 tools**：处理 `none`/`auto`/`required`/`function` 类型
5. **注入 create_agent_task 特有字段**：
   - `user_input`: `{"id": uuid, "text": user_text}`
   - `model_name`: 通过 `model_name_map()` 映射
   - `agent_type`/`agent_id`: 与 `function` 相同
   - `conversation_id`/`session_id`/`project_id`: 每次生成新 UUID
   - `user_id`/`device_id`/`machine_id`: 账号专属信息
   - `summary_config`/`chat_memory_config`: 禁用
   - `is_new_session`: true

### 9.4 SSE 解析（sse.rs）

SSE 解析器处理两种接口的事件格式：

**解析器核心**：`parse_solo_line` 函数根据事件类型提取不同字段：

```rust
match ev.event.as_str() {
    "output" => {
        // llm_utils_chat: response 字段 → content
        ev.response = obj.get("response")...
    }
    "thought" => {
        // create_agent_task: thought 字段 → content
        ev.response = obj.get("thought")...
    }
    "turn_completion" => {
        // create_agent_task: 替代 done 事件
        ev.finish_reason = "stop"
    }
    "done" => { ... }
    "token_usage" => { ... }
    "error" => { ... }
    _ => {} // 忽略 task_created, model_config, history 等
}
```

**流式转换**：`stream_convert` 函数将 SOLO SSE 事件逐个转为 OpenAI chunk 格式：

```
SOLO event:thought {"thought":"Hi!"} 
    → 
OpenAI data: {"choices":[{"delta":{"content":"Hi!"}}]}
```

**非流式聚合**：`aggregate` 函数将所有 thought 事件的 `thought` 字段拼接为完整回复。

### 9.5 关键常量

```rust
// mod.rs
pub const AGENT_HOST: &str = "https://api5-normal.mchost.guru";
pub const EP_CREATE_TASK: &str = "/api/agent/v3/create_agent_task";
pub const APP_ID: &str = "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8";
pub const IDE_VERSION: &str = "0.1.50";
pub const IDE_VERSION_CODE: &str = "20260811";
pub const FUNCTION: &str = "solo_work_lite";
pub const DEFAULT_MODEL: &str = "glm-5.2";
pub const REFERER_BASE: &str = "https://trae-api-cn.mchost.guru";
```

---

## 10. 账号池与健康检查逻辑

### 10.1 账号选择策略

`pick_excluding` 函数按以下优先级选择账号：

1. **过滤条件**（跳过不满足的账号）：
   - 已尝试过的账号（`tried` 集合）
   - 被禁用的账号（`disabled = true`，SessionDead 导致）
   - 处于冷却期的账号（`until > now`）
   - 积分已过期的账号（`credits_expire_at > 0 && credits_expire_at < now`）
   - 零积分账号（`credits <= 0`）

2. **优先级排序**（选择最优账号）：
   - 优先选择有积分过期时间的账号（即将过期的优先使用）
   - 过期时间相同 → 选择积分更多的账号
   - 无过期时间的账号排最后

3. **轮换策略**：
   - 最多轮换 3 次（`MAX_ROTATE = 3`）
   - 连接级错误（DNS/超时/TLS）→ 冷却并轮换到下一账号
   - 流式中间错误 → 冷却但不轮换（流已开始，无法切换）

### 10.2 device_id 与 machine_id 生成

每个账号有独立的虚拟设备标识，从 `device_map.json` 读取：

```rust
// pool.rs - sync_from_accounts
let (device_id, machine_id) = device_map
    .get(uid)
    .map(|d| (d.device_id.clone(), seeded_hex(64, uid, "mach")))
    .unwrap_or_else(|| (String::new(), seeded_hex(64, uid, "mach")));
```

`machine_id` 通过 `seeded_hex` 函数从 `uid` 确定性派生（SHA256 迭代哈希），保证同一账号始终得到同一机器标识。

### 10.3 积分类型变更影响

切换到 `create_agent_task` 后：
- 账号池中的 `credits` 字段现在对应 **Work 积分**（product_id 209）
- 每日签到获得的 Work 积分可以直接用于 API 调用
- IDE 积分（product_id 208）不再影响 API 服务可用性
- 之前因 IDE 积分耗尽被跳过的账号（有 Work 积分）现在变为可用

---

## 11. 错误分类与冷却策略

### 11.1 错误分类（ErrKind）

| 错误类型 | 触发条件 | 冷却时长 | 说明 |
|----------|----------|----------|------|
| `PlanLimit` | code=1005 或 message 含 "plan" | 12 小时 | 套餐额度用尽 |
| `SoftRate` | code=4008 或 429 或含 "quota"/"rate" | 60 秒 | 请求频率超限 |
| `SessionDead` | HTTP 401 或 code=401 | 24 小时（禁用） | JWT 过期/会话死亡 |
| `NotFound` | HTTP 404 | 60 秒 | 资源不存在 |
| `Server` | HTTP 5xx | 10 分钟 | 服务器错误 |
| `Client` | HTTP 4xx（其他） | 10 分钟 | 客户端错误 |
| `consecutive_errors` | 连续 3 次错误 | 10 分钟 | 连续错误冷却 |

### 11.2 错误分类函数

```rust
// HTTP 状态码分类
pub fn classify_error(status: u16, body: &str) -> ErrKind
// SOLO 业务错误码分类（流内 error 事件）
pub fn classify_solo_error(code: i64, msg: &str) -> ErrKind
```

### 11.3 错误码 4008 特殊处理

错误码 4008（请求频率限制）使用 60 秒短冷却而非 12 小时长冷却，避免误杀：

```rust
if code == 4008 || msg_lower.contains("quota") || msg_lower.contains("exceeded") {
    return ErrKind::SoftRate; // 60 秒冷却
}
```

---

## 12. 模型名映射表

`create_agent_task` 请求体需要 `model_name` 字段（内部标识），通过 `model_name_map` 函数从显示名映射：

| 显示名 (model/config_name) | 内部标识 (model_name) |
|---------------------------|----------------------|
| `DeepSeek-V4-Flash` | `deepseek_v4_flash__dev` |
| `DeepSeek-V4-Flash-Official` | `deepseek_v4_flash__dev` |
| `DeepSeek-V4-Pro` | `deepseek_v4_pro__dev` |
| `glm-5.2` | `glm_52__dev` |
| `glm-5-turbo` | `glm_5_turbo__dev` |
| `glm-5` | `glm_5__dev` |
| `Doubao-Seed-2.1-Pro` | `doubao_seed_21_pro__dev` |
| `seed-code-pro-0430` | `doubao_seed_21_pro__dev` |
| `Doubao-Seed-2.1-Turbo` | `doubao_seed_21_turbo__dev` |
| `Doubao-Seed-2.0-Code` | `doubao_seed_20_code__dev` |
| `kimi-k3` | `kimi_k3__dev` |
| `kimi-k2.7-code` | `kimi_k27_code__dev` |
| `kimi-k2.6` | `kimi_k26__dev` |
| `minimax-m3` | `minimax_m3__dev` |
| `qwen-3.7-plus` | `qwen_37_plus__dev` |
| `sagitta` | `sagitta__dev` |
| `aquila` | `aquila__dev` |
| 其他/未知 | `deepseek_v4_flash__dev`（默认） |

---

## 附录 A：代理环境变量

API 服务启动时必须设置 `NO_PROXY=*` 环境变量，防止 `ureq` HTTP 客户端走系统代理（127.0.0.1:8899）形成循环：

```rust
// 启动 API 服务时设置
std::env::set_var("NO_PROXY", "*");
```

不设置时会导致：
- 请求 `api5-normal.mchost.guru` 被系统代理拦截
- 代理再次请求同一地址 → 形成循环
- 10 秒超时 → 触发 `consecutive_errors` 冷却

## 附录 B：SSE 流式超时配置

```rust
// mod.rs - streaming_agent()
ureq::AgentBuilder::new()
    .timeout_write(Duration::from_secs(30))   // 写超时 30s
    .timeout_connect(Duration::from_secs(10)) // 连接超时 10s
    // 不设置 timeout_read（SSE 流式需要无读超时）
    .max_idle_connections(20)
    .max_idle_connections_per_host(20)
    .build()
```

> **注意**: `timeout_read(Duration::from_secs(0))` 会触发 Rust std 的 "cannot set a 0 duration timeout" 错误，绝对不能使用。

## 附录 C：代理 SSE 流式转发修复

项目 MITM 代理（`device_proxy.py`）中 `forward_upstream` 函数原使用 `resp.read()` 全量缓冲 SSE 响应，导致客户端超时。修复方案为新增 `_stream_response` 函数，使用 `Transfer-Encoding: chunked` 逐块转发：

```python
def _stream_response(resp, resp_headers, client_sock, ...):
    """逐块读取 SSE 响应并转发，避免全量缓冲导致超时"""
    # 使用 chunked encoding 逐块转发
    while True:
        chunk = resp.read(8192)
        if not chunk:
            break
        client_sock.sendall(f"{len(chunk):x}\r\n".encode("ascii") + chunk + b"\r\n")
    client_sock.sendall(b"0\r\n\r\n")
```

触发条件：请求头包含 `accept: text/event-stream` 或 `accept: event-stream`。

## 附录 D：create_agent_task 加密问题调研记录（2026-08-14）

### 背景

项目原计划从 `llm_utils_chat`（IDE 积分 product_id=208）切换到 `create_agent_task`（Work 积分 product_id=209），因所有账号 IDE 积分已耗尽而 Work 积分充足。切换后发现 `create_agent_task` 端点返回 `4001` 错误，经详细排查确认服务端已更新为强制要求 aha 加密传输。

### 测试结果

| 测试组合 | HTTP 状态 | 响应 |
|----------|-----------|------|
| `create_agent_task` + `summary_config:{enabled:false}` | 200 (SSE) | `event:error code:4001 msg:failed to get summary config` |
| `create_agent_task` 无 `summary_config` | 200 (SSE) | 同上 |
| `create_agent_task` + `summary_config:{enabled:true}` | 200 (SSE) | 同上 |
| `create_agent_task` + `x-bridge-transport:aha` | 200 (SSE) | 同上 |
| `create_agent_task` 换账号（account 0/1） | 200 (SSE) | 同上（非账号问题） |
| `workflow/start` 同 body | 400 | `missing required parameter workflow_info` |
| `llm_utils_chat`（对照组） | 200 (SSE) | 正常 SSE 事件流（但 IDE 积分为 0） |

### 根因分析

通过 MITM 代理日志（`test_proxy_20260814_212432.log`）对比真实 Trae 客户端请求：

1. **请求体完全加密**：真实客户端发送 129KB 加密请求体（aha/TTNet 加密层），非明文 JSON
2. **安全头必需**：请求携带 `x-helios`、`x-medusa`、`x-neptune`、`x-request-pin`、`x-requested-at` 等安全头，由 `@aha-kit/net` 原生模块生成
3. **服务端行为变更**：本文档 3.3 节记载的"不带 `x-bridge-transport: aha` 头时服务器直接解析明文 JSON"**已失效**。服务端在解析请求体之前即尝试获取 summary 配置，明文请求无法通过此校验

### Aha 加密模块调研

- **原生模块**：`@aha-kit/net`（NAPI 绑定），入口 `wrapper.js` → `target/nodejs/index.js` → `index.win32-x64-msvc.node`
- **初始化配置**：需 `ttnetLibPath`（指向 `sscronet.dll`）、`proxy`、`ttnetParams`（appId/deviceId/domainHttpdns 等）、`params`（storagePath/userAgent/enableHttp2 等）
- **init panic**：使用 `sscronet.dll` 作为 `ttnetLibPath` 时，`nativeBinding.init(config, reporter)` 触发 `Panic during init: Any { .. }`，原因未定位
- **CN 区域配置差异**：appId 应为 `"801256"`（非 i18n 的 `"573653"`），`enableCaStore` 应为 `true`，`appName` 为 `"TraePlugin"`（来源：`extensions/ai-completion/dist/extension.js` 中 `getTTNetConfigByRegion()`）
- **结论**：aha 原生模块初始化问题未解决，无法在测试脚本中模拟加密请求

### Trae 客户端实际调用路径

通过代理日志和代码分析确认：

1. `api/agent/v3/create_agent_task` — 由 Trae Work 原生 Rust 二进制（`modules/ai-agent/ai_agent.dll`）调用，请求体加密
2. `api/cue_agent/v3/create_agent_task` — 由 `cueMain.js` 中 `CueAgentService` 调用，用于内联代码补全（非 SOLO Work），请求体格式完全不同（含 `render_context`、`biz_context` 等字段）
3. `api/agent/v3/workflow/start` — 由 Trae Work 原生二进制调用，需 `workflow_info` 参数，用于内部工作流（如 `summarize_message`）

### 模型名格式修正

代理日志 SSE 响应中 `model_config` 事件确认：`model_name` 使用**连字符**格式（如 `glm-5.2__dev`），非本文档第 12 节记载的下划线格式（`glm_52__dev`）。第 12 节映射表有误，实际值以代理日志为准。

### 决策

由于 `create_agent_task` 的 aha 加密问题短期内无法解决，项目 API 服务**回退至 `llm_utils_chat` 模式**（消耗 IDE 积分）。后续如需使用 Work 积分，需先解决 aha 原生模块初始化问题。

### 待调研项

- aha 原生模块 init panic 的根因（可能是 `sscronet.dll` 版本不匹配或配置字段缺失）
- 是否可通过 Electron 进程内调用的方式使用 aha 加密（Trae 主进程已通过 `electron.ahaNet` 初始化）
- `workflow/start` 端点的完整请求体格式（需反编译 `ai_agent.dll` 或抓取更多流量）
