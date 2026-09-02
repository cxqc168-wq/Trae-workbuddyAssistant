# Trae Work Assistant 功能增强需求计划

> **版本**: v2.4.2 | **状态**: 已完成 | **完成日期**: 2026-08-15
>
> Phase 1（1.1-1.4）、Phase 2（2.1-2.5）、Phase 3（3.1-3.5）全部实现并验证通过。
>
> 基于本项目产品需求与技术分析，整理出以下功能增强需求，按难度和功能相关度分三个阶段排列。

---

## 一、项目现状

| 维度 | 当前状态 |
|------|---------|
| 技术栈 | Tauri (Rust) + React + TypeScript + Python 代理 |
| 核心功能 | 账号管理、自动签到、MITM 代理（JWT 捕获 + 设备改写）、代理日志 |
| Token 管理 | 从代理流量捕获 JWT，13 天过期后需手动重新抓取 |
| 签到策略 | 所有账号一视同仁，无错误分类、无冷却、无自动解冻 |
| 对话流量 | `trae-api-cn.mchost.guru`（TRAE 核心对话域名）未在监听列表，代理不解析 |
| 账号调度 | 无积分感知、无优先级策略 |

---

## 二、需求总览

| 阶段 | 时间预估 | 需求数 | 定位 |
|------|---------|--------|------|
| Phase 1 | 1-2 天 | 4 项 | 短期优化：修补现有缺陷，增强稳定性 |
| Phase 2 | 1-2 周 | 5 项 | 中期增强：Token 续期、智能调度、设备防关联 |
| Phase 3 | 2-4 周 | 5 项 | 长期扩展：OpenAI 兼容 API、SSE 转换、账号池 |

---

## Phase 1：短期优化（1-2 天，低难度）

### 1.1 添加 mchost.guru 到默认监听域名

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 低 |
| 优先级 | 高 |
| 涉及文件 | `models.rs` `default_proxy_domains()`、`Settings.tsx` |

**背景**：TRAE SOLO CN 的核心对话/大模型交互走 `https://trae-api-cn.mchost.guru/api/agent/v3/llm_utils_chat`，采用 HTTP POST + SSE 流式响应。当前该域名不在监听列表中，代理只做透明隧道转发，无法解析对话流量。

**需求描述**：
- 将 `mchost.guru` 添加到 `default_proxy_domains()` 默认列表
- 代理对该域名做 MITM 解密，在代理日志中记录对话请求
- 系统设置页 placeholder 同步更新

**验收标准**：
- TRAE 发起对话时，代理日志中出现 `mchost.guru` 的请求记录
- 对话功能不受影响（TRAE 信任代理 CA 证书）

---

### 1.2 签到错误分类冷却状态机

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 低 |
| 优先级 | 高 |
| 涉及文件 | `accounts.rs`、`store.ts`、`Checkin.tsx` |

**背景**：当前签到失败后无差异化处理。不同错误类型应采用不同冷却策略，避免无意义重试或过度惩罚。

**需求描述**：

按 HTTP 状态码和响应体内容分类签到错误，每种类型对应不同冷却策略：

| 错误类型 | 触发条件 | 处理方式 | 冷却时长 |
|---------|---------|---------|---------|
| `PlanLimit` | 响应体含 `code:1005` | 长冷却（额度不足，短期不会恢复） | 12 小时 |
| `SoftRate` | HTTP 429 | 短冷却（限流） | 60 秒 |
| `SessionDead` | HTTP 401 | 标记需重新登录，不自动重试 | - |
| `NotFound` | HTTP 404 | 短冷却，不累计错误计数 | 60 秒 |
| `Server` | HTTP 5xx | 累计错误，达阈值后冷却 | 10 分钟 |
| `Client` | 其他 4xx | 累计错误，达阈值后冷却 | 10 分钟 |

**验收标准**：
- 签到失败时，账号列表展示冷却状态和剩余时间
- 冷却中的账号在一键签到时自动跳过
- 冷却到期后自动恢复可签到状态

---

### 1.3 签到自动解冻闭环

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 低 |
| 优先级 | 中 |
| 涉及文件 | `accounts.rs`、`auto_checkin.py` |

**背景**：账号积分耗尽后进入冷却，次日签到补充积分后应自动恢复，无需人工干预。

**需求描述**：
- 签到成功后自动调用剩余积分查询接口
- 若账号有可用积分（remaining > 0）且当前处于冷却状态，自动清除冷却
- 形成"额度耗尽冷却 → 次日签到补充 → 自动恢复"的自愈闭环

**验收标准**：
- 冷却中的账号签到成功后，冷却状态自动清除
- 账号列表实时更新冷却/可用状态

---

### 1.4 多策略 TRAE 安装路径探测

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 低 |
| 优先级 | 中 |
| 涉及文件 | `trae-switch-bridge.ps1`、Rust 侧进程管理 |

**背景**：当前项目硬编码 TRAE 安装路径，换盘符或非默认安装目录时失效。

**需求描述**：

按优先级依次尝试 5 种策略探测 TRAE SOLO CN 安装路径：

1. 运行中进程的 `Path` 属性
2. 常见路径多盘符扫描（`C:\`、`D:\` 等 + `Programs\TRAE SOLO CN\`）
3. 桌面/开始菜单 `.lnk` 快捷方式解析
4. 注册表卸载项 `HKLM:\...\InstallLocation`
5. `%LOCALAPPDATA%\Programs\TRAE SOLO CN\`

**验收标准**：
- TRAE 安装在非默认路径时，启动/停止/重启功能正常工作
- 路径探测结果缓存，避免重复扫描

---

## Phase 2：中期增强（1-2 周，中难度）

### 2.1 Token 自动刷新（refresh_token → ExchangeToken）

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 中 |
| 优先级 | 高 |
| 涉及文件 | 新增 `jwt.rs` 刷新逻辑、`accounts.rs`、Python 代理捕获扩展 |

**背景**：当前从代理流量捕获的 JWT 只有 accessToken，13 天过期后必须手动重新抓取。本项目规划通过 OAuth 登录获取 refresh_token，过期前 24h 自动调 ExchangeToken 续期。

**需求描述**：

分两步实现：

**步骤一：从代理流量捕获 refresh_token**
- 扩展 Python 代理的 JWT 捕获逻辑，同时捕获 `refresh_token` 字段（如果请求体或响应体中包含）
- 在 `checkin_accounts.json` 中增加 `refresh_token` 字段存储
- 代理捕获到新 refresh_token 时自动更新（带版本号防降级）

**步骤二：自动刷新 accessToken**
- 在 `accounts.rs` 中实现 `refresh_jwt` 命令，调用 `POST https://api.trae.com.cn/cloudide/api/v3/trae/oauth/ExchangeToken`
- 请求体：`{"ClientID":"en1oxy7wnw8j9n","RefreshToken":"<refresh_token>","ClientSecret":"-","UserID":""}`
- 过期前 24h 自动触发刷新（在 `build_account_views` 中检查 `jwt_exp_timestamp`）
- refresh_token 每次刷新后轮换，旧 token 失效，需原子写回
- 并发安全：持锁重查防重复刷新（多个并发请求不能同时 ExchangeToken）

**验收标准**：
- 账号添加后 13 天内无需手动操作，JWT 自动续期
- 刷新失败时回退旧 token，不改写字段
- 前端展示 JWT 剩余有效期和自动刷新状态

---

### 2.2 积分过期感知调度

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 中 |
| 优先级 | 中 |
| 涉及文件 | `accounts.rs`、`auto_checkin.py` |

**背景**：签到对所有账号一视同仁。应优先签到积分即将过期的账号，避免积分浪费。

**需求描述**：

在签到调度中引入积分过期感知：

1. 从剩余积分查询接口的响应中提取每个权益包的 `expire_time`
2. 取最近过期的 `expire_time` 作为该账号的"积分过期时间"
3. 签到顺序按过期时间升序排列（最近过期的优先签到）
4. 过期时间相同的账号按剩余积分降序排列

**验收标准**：
- 一键签到时，积分即将过期的账号排在前面
- 账号列表展示积分过期时间列
- 过期时间 < 24h 的账号高亮提醒

---

### 2.3 双 HTTP Client 设计

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 中 |
| 优先级 | 中 |
| 涉及文件 | `accounts.rs` HTTP 请求逻辑 |

**背景**：当前所有 HTTP 请求（签到、积分查询、Token 刷新）使用相同的超时配置。未来如果支持 SSE 流式对话，短请求和长流式请求需要分离的超时策略。

**需求描述**：
- 短请求 Client：总超时 120s，用于签到/积分查询/Token 刷新等 JSON 请求
- 流式 Client：无总超时，仅 `ResponseHeaderTimeout: 120s`，用于 SSE 流式对话
- 两者共享连接池（`MaxIdleConnsPerHost: 20`）
- 在 `ureq` 中通过不同的 `Agent` 配置实现

**验收标准**：
- 短请求不会因超时配置不当而失败
- 流式请求不会被总超时截断
- 连接池复用，无连接泄漏

---

### 2.4 代理日志 SSE 流摘要展示

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 中 |
| 优先级 | 中 |
| 涉及文件 | `device_proxy.py` `forward_upstream`、`ProxyRequestLogger`、`misc.rs`、`Logs.tsx` |

**背景**：添加 `mchost.guru` 到监听域名后，代理会捕获 TRAE 对话请求。但 SSE 流式响应的完整内容太大，不适合全量记录。应提取摘要信息。

**需求描述**：

在代理日志中为 `llm_utils_chat` 请求增加摘要信息：

1. 请求侧：记录 `model`、`config_name`、`messages` 数量、首条消息前 100 字符
2. 响应侧：解析 SSE 流，提取 `token_usage`（prompt_tokens / completion_tokens / total_tokens）、`finish_reason`
3. 不记录完整对话内容（隐私 + 体积），只记录元数据摘要
4. 代理日志详情弹窗中展示摘要信息

**SSE 事件格式参考**（本项目协议分析）：
```
event:metadata        → 会话元数据
event:output          → ×N，增量内容（response/reasoning_content/tool_calls）
event:token_usage     → token 统计
event:done            → 结束信号
```

**验收标准**：
- 代理日志中 `mchost.guru` 的请求展示模型、token 用量摘要
- 不记录完整对话内容
- 详情弹窗展示结构化摘要信息

---

### 2.5 6 层设备标识重置

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 中 |
| 优先级 | 低 |
| 涉及文件 | `trae-switch-bridge.ps1`、新增 Rust 命令 |

**背景**：当前仅通过 MITM 代理改写签到接口的 `x-device-id`。对于新注册账号场景，需要从文件层重置设备标识，防止多账号被服务端关联。

**需求描述**：

实现 6 层设备标识重置，在"新建账号"流程中调用：

1. `machineid` 文件 — 替换为新的 hex32 UUID
2. `storage.json` 中 `telemetry.machineId` / `telemetry.sqmId` — 替换
3. `storage.json` 中 `aha.device.device_id` — 替换
4. `aha/TinyStorage` 中 `device_id` — 清除（强制重新注册）
5. 注册表 `HKLM:\SOFTWARE\Microsoft\Cryptography\MachineGuid` — 替换
6. `trae-webview` 追踪数据（Cookies/Local Storage/Session Storage）— 清除

额外：删除 `has_device_id_updated_to_aha` 标记位，否则 TRAE 不会重新注册设备。

**验收标准**：
- "新建账号"功能执行后，6 层标识全部重置
- TRAE 启动后自动用新设备 ID 注册
- 不影响已有账号的登录态

---

## Phase 3：长期扩展（2-4 周，高难度）

### 3.1 OpenAI 兼容 API 端点

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 高 |
| 优先级 | 中 |
| 涉及文件 | 新增 `api_server` 模块（Rust axum/actix 或 Python FastAPI） |

**背景**：本项目将 TRAE SOLO 的免费对话通道包装成标准 OpenAI 兼容接口，扩展为额度管理 + API 网关。

**需求描述**：

在项目中嵌入一个 OpenAI 兼容的 HTTP API 服务：

1. `POST /v1/chat/completions` — 对话接口（流式 + 非流式）
2. `GET /v1/models` — 模型列表
3. `GET /status` — 账号池状态
4. `GET /health` — 健康检查
5. Bearer API Key 鉴权（常量时间比较，防时序攻击）
6. 请求体限制 8MB

**请求体改写**（OpenAI → SOLO 格式）：

| 改写项 | OpenAI 格式 | SOLO 格式 |
|--------|-------------|-----------|
| messages.content | 字符串 | 数组 `[{"type":"text","text":"..."}]` |
| stream | 可选 | 强制 true |
| model | `"glm-5.2"` | 拆成 `config_name` + `model` |
| function | 无 | 固定 `"solo_work_lite"` |

**验收标准**：
- 使用 OpenAI SDK 可直接调用，无需修改客户端代码
- 支持流式和非流式两种模式
- API Key 鉴权生效

---

### 3.2 SSE 协议转换

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 高 |
| 优先级 | 中（依赖 3.1） |
| 涉及文件 | 新增 SSE 转换模块 |

**背景**：TRAE SOLO 的 SSE 不是标准 OpenAI 格式，而是自定义事件类型。需要解析并转换为标准 OpenAI `chat.completion.chunk` 格式。

**需求描述**：

实现 SOLO SSE → OpenAI SSE 的双向转换：

**SOLO SSE 事件序列**：
```
event:metadata        → 会话元数据（忽略）
event:timing_cost     → 耗时统计（忽略）
event:output          → ×N，增量内容
  data: {"response":"<content>", "reasoning_content":"<思考链>", "tool_calls":...}
event:extra_info      → 额外信息（忽略）
event:token_usage     → token 统计
event:done            → 结束信号
```

**转换为 OpenAI chunk**：
```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion.chunk",
  "choices": [{"index":0, "delta":{"content":"增量文本"}, "finish_reason":null}]
}
```

**关键处理**：
- `output` 事件 → `delta.content` / `delta.reasoning_content` / `delta.tool_calls`
- `token_usage` 事件 → 附加到最后一个 chunk 的 `usage` 字段
- `done` 事件 → 写 `finish_reason` + `data: [DONE]`
- `error` 事件 → 回调冷却账号 + 注入错误事件
- 上游中断无 `done` → 幂等兜底仍写 `[DONE]`

**tool_calls 字段转换**：
- `function_call` → `function` 字段名转换
- 清理 `namespace`、`partial_arguments` 等 SOLO 专属字段
- `arguments` 增量拼接

**验收标准**：
- OpenAI SDK 客户端收到的流式响应格式正确
- tool_calls 功能正常
- 上游中断时客户端收到 `[DONE]` 不会挂起

---

### 3.3 账号池智能调度

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 高 |
| 优先级 | 中（依赖 3.1） |
| 涉及文件 | 新增 `pool` 模块 |

**背景**：多账号场景下，需要智能选择最优账号处理 API 请求，最大化利用免费额度。

**需求描述**：

实现账号池管理器，包含以下能力：

**智能挑选策略**（`PickExcluding`）：
1. 跳过禁用/冷却中/积分已过期/零积分账号
2. `creditsExpireAt` 非零者优先（有过期时间的账号）
3. 过期时间升序（最近过期的优先用，避免积分浪费）
4. 过期时间相同 → 积分降序

**错误分类联动**（与 Phase 1.2 共用状态机）：

| 错误类型 | 账号池处理 |
|---------|-----------|
| `PlanLimit` | 长冷却 12h，换号重试 |
| `SoftRate` | 短冷却 60s，换号重试 |
| `SessionDead` | 永久禁用（需重登），换号重试 |
| `NotFound` | 短冷却 60s，不累计错误，换号重试 |
| `Server` / `Client` | 累计错误，达阈值冷却 10m |

**单请求最多换号次数**：默认 3 次（`MaxRotate`）

**状态持久化**：冷却状态、积分、过期时间、禁用原因存入 `api_pool.json`，重启不丢失

**验收标准**：
- API 请求自动选择最优账号
- 单账号额度耗尽时自动切换到下一个可用账号
- 重启后冷却状态保持

---

### 3.4 登录态快照备份/恢复

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 高 |
| 优先级 | 低 |
| 涉及文件 | `trae-switch-bridge.ps1`、新增 Rust 命令、前端账号管理页 |

**背景**：当前通过 JWT 管理账号，但 JWT 过期且无法自动刷新时，需要手动重新登录。登录态快照可以实现"免验证码秒切"。

**需求描述**：

实现全量文件级账号切换：

**备份的文件清单**：
- `storage.json`（globalStorage，含 telemetry.machineId / aha.device.device_id）
- `machineid`（设备标识）
- `aha/`（aha 设备注册数据）
- `Preferences` / `Local State`
- `Local Storage/leveldb/`（Chromium 本地存储）
- `Network/`（Cookies / 会话）
- `Partitions/trae-webview/`（登录 Webview）
- `Session Storage/`
- `state.vscdb`（VS Code 状态库）

**工作流**：
1. 切换前：关闭 TRAE → 备份当前账号 profile → 恢复目标账号 profile → 启动 TRAE
2. 新建账号：关闭 TRAE → 重置设备 ID（Phase 2.5）→ 备份空 profile → 启动 TRAE → 用户登录 → 保存 profile

**数据存储**：`%APPDATA%\TraeWorkAssistant\profiles\<account_id>\`

**验收标准**：
- 账号切换无需重新输入验证码
- 切换后 TRAE 登录态完整恢复
- 切换过程 < 10 秒

---

### 3.5 OAuth 登录闭环

| 属性 | 值 |
|------|-----|
| 来源 | 本项目自研设计 |
| 难度 | 高 |
| 优先级 | 低 |
| 涉及文件 | 新增 OAuth 登录模块、前端登录页 |

**背景**：本项目可独立构造 OAuth 登录 URL，引导用户浏览器登录后从回调直接拿 refresh_token，无需依赖 TRAE 桌面端。这种方式可以跳过"从本地存储逆向提取 token"的所有难题。

**需求描述**：

实现独立的 OAuth 登录流程：

1. 用 `openssl rand -hex 16` 生成新的 `machine_id` / `device_id`
2. 构造登录 URL：`https://www.trae.cn/authorization?client_id=en1oxy7wnw8j9n&...&auth_callback_url=http://127.0.0.1:<port>/authorize`
3. 用户在浏览器登录后，复制回调链接粘贴到应用
4. 从回调 URL 解析 `refreshToken` 和 `userInfo`
5. 调用 `ExchangeToken` 换取 `accessToken`
6. 调用 `GetUserInfo` 获取账号信息
7. 落盘到 `checkin_accounts.json`（含 refresh_token）
8. 自动签到 + 查积分

**与 Phase 2.1 的关系**：OAuth 登录闭环是获取 refresh_token 的根本途径。Phase 2.1 的"从代理流量捕获"是过渡方案，OAuth 登录是最终方案。

**验收标准**：
- 不需要打开 TRAE 桌面端即可添加账号
- 添加的账号自带 refresh_token，支持自动续期
- 每个账号天然有独立的 machine_id / device_id

---

## 三、需求依赖关系

```
Phase 1:
  1.1 mchost.guru 监听 ──────────────────────┐
  1.2 签到错误分类冷却 ──────────────┐       │
  1.3 签到自动解冻（依赖 1.2）◄──────┘       │
  1.4 多策略路径探测                            │
                                               │
Phase 2:                                       │
  2.1 Token 自动刷新                           │
  2.2 积分过期感知调度（依赖 1.2）             │
  2.3 双 HTTP Client 设计                      │
  2.4 SSE 流摘要展示（依赖 1.1）◄──────────────┘
  2.5 6 层设备标识重置

Phase 3:
  3.1 OpenAI 兼容 API 端点（依赖 2.3）
  3.2 SSE 协议转换（依赖 3.1）
  3.3 账号池智能调度（依赖 1.2 + 2.2）
  3.4 登录态快照备份/恢复（依赖 2.5）
  3.5 OAuth 登录闭环（可替代 2.1 步骤一）
```

---

## 四、TRAE SOLO CN API 域名与协议参考

| 域名 | 端点 | 协议 | 用途 | 当前代理状态 |
|------|------|------|------|-------------|
| `trae-api-cn.mchost.guru` | `POST /api/agent/v3/llm_utils_chat` | HTTP + SSE | 核心对话/大模型交互 | 未监听（Phase 1.1 添加） |
| `trae-api-cn.mchost.guru` | `POST /api/ide/v1/get_detail_param` | HTTP + JSON | 模型列表 | 未监听 |
| `api.trae.cn` | `POST /trae/api/v2/ug/checkin_credits/claim` | HTTP + JSON | 执行签到 | 已监听 (trae.cn) |
| `api.trae.cn` | `POST /trae/api/v2/ug/checkin_credits/status` | HTTP + JSON | 签到状态 | 已监听 |
| `api.trae.cn` | `POST /trae/api/v2/pay/ide_user_ent_usage` | HTTP + JSON | 积分/权益查询 | 已监听 |
| `api.trae.com.cn` | `POST /cloudide/api/v3/trae/oauth/ExchangeToken` | HTTP + JSON | Token 刷新 | 已监听 (trae.com.cn) |
| `api.trae.com.cn` | `POST /cloudide/api/v3/trae/GetUserInfo` | HTTP + JSON | 用户信息 | 已监听 |
| `www.trae.cn` | `GET /authorization` | HTTPS | 登录授权页 | 已监听 |

**认证方式**：`Authorization: Cloud-IDE-JWT <accessToken>`，附带 `X-Cloudide-Token`、`X-Ide-Token`、`X-App-Id`、`X-Ide-Version`、`X-Device-Id` 等请求头。

**SOLO SSE 自定义事件格式**：
```
event:metadata        → 会话元数据（session_id, model 等）
event:timing_cost     → 耗时统计
event:output          → ×N，增量内容（response / reasoning_content / tool_calls）
event:extra_info      → 额外信息
event:token_usage     → token 统计（prompt_tokens / completion_tokens / total_tokens）
event:done            → 结束信号（finish_reason）
event:error           → 流内错误（code:1005 等）
```

---

## 五、风险与注意事项

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 添加 `mchost.guru` 后 MITM 可能影响 TRAE 对话 | TRAE 对话功能异常 | 确保 CA 证书已安装；代理日志不记录完整对话内容 |
| Token 自动刷新失败 | 账号无法自动续期 | 失败时回退旧 token，不改写字段；前端提醒手动处理 |
| OAuth 登录闭环需构造合法请求头 | 登录失败 | 确保 ClientID/AppID 正确（本项目协议实测） |
| OpenAI 兼容 API 需完整逆向 SOLO 协议 | 功能不完整 | 分步实现：先非流式，再流式；先基础对话，再 tool_calls |
| 6 层设备重置需管理员权限 | 部分操作失败 | 注册表操作需提权；文件操作在用户目录下无需提权 |
| SSE 流式响应可能产生大量日志 | 磁盘空间不足 | 仅记录摘要元数据，不记录完整内容；代理日志 100MB 滚动 |
