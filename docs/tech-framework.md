# 技术框架方案 — Trae Work 助手 v2.4.4

> 详细界面/交互/数据模型见 `product-design.md`。本文档给出技术选型、架构、进程契约与风险。
> v2.0 新增：本地 API 网关（axum）、SSE 协议转换、账号池智能调度、签到错误冷却状态机、6 层设备标识重置。
> v2.1 新增：每日积分快照（total/earned/consumed 三线趋势）、暗色模式图表适配、Mutex 安全锁（poison 恢复）、代理日志竞态修复。

## 1. 技术选型

| 层 | 选型 | 理由 |
|---|---|---|
| UI | React 18 + TypeScript + Tailwind CSS + shadcn/ui + Recharts + Zustand | Web 技术栈可完整还原设计稿；状态用 Zustand；图表用 Recharts |
| 外壳 | Tauri 2.x（Rust） | 包体 8~15MB（远小于 Electron），可调用系统 API（注册表/证书/计划任务） |
| 核心逻辑 | Python 3.13（`auto_checkin.py` / `device_proxy.py`，迁移并增强） | 复用已验证的签到/代理逻辑，降低重写风险 |
| 登录态切换 | PowerShell（`trae-switch-bridge.ps1`，含 6 层设备标识重置） | 复用已验证的备份/恢复/机器码重置逻辑 |
| API 网关 | Rust axum（内嵌，复用 Tauri tokio runtime） | OpenAI 兼容端点 + SSE 转换 + 账号池调度，无需独立进程 |
| HTTP 客户端 | ureq（同步）+ spawn_blocking 包装 | 双 Client 设计：短请求 120s 超时 / 流式仅 ResponseHeaderTimeout |
| 打包 | Tauri Bundler → MSI / NSIS 单文件 | 含 Python embeddable 运行时与 PS 脚本 |

**不采用**：Electron（体积过大）、WPF/WinUI（样式成本高）、PyQt（视觉不达要求）。

## 2. 架构分层

```
Presentation  React + Tailwind（概览/账号/签到/积分/日志/设置/API 服务）
      │  Tauri invoke
State         Zustand + Tauri Event Bus（账号/任务/日志/环境）
      │
Bridge        Tauri Commands (Rust)
 ├─ fs         JSON 读写（tmp+rename 原子替换 + 文件锁）
 ├─ proc       子进程管理（spawn/kill/stdout 流）
 ├─ sys        TW 检测 / CA 检测与安装 / UAC / 计划任务
 ├─ jwt        JWT 解析（exp / data.id），不校验签名
 ├─ api_server axum 内嵌 HTTP 服务（/v1/chat/completions + SSE 转换）
 ├─ pool       账号池调度（积分过期感知 + 冷却状态机 + 轮转）
 └─ watch      文件监听（accounts.json 变更→推事件）
      │
Python Core   auto_checkin.py（签到 + 错误分类冷却） / device_proxy.py（代理 + mchost.guru 监听）
PowerShell     trae-switch-bridge.ps1（登录态切换 + 6 层设备标识重置）
```

v2.0.0 架构变更：API 网关从「下期独立模块」改为内嵌 axum 服务，复用 Tauri 的 tokio runtime，无需独立进程。SSE 转换通过 `spawn_blocking` 包装 ureq 同步请求实现。

### 2.1 前端实现要点

- **启动加载态**：`App.tsx` 在 store `ready` 为 `false` 时渲染全局 Loading（旋转指示器 + 「正在加载…」），待 `init()` 完成后才挂载主界面，避免空状态闪烁。
- **设置页（显式保存）**：Settings 页使用本地表单状态（`form`），与后端 `settings` 对比得到 `dirty` 标记；用户编辑后需点击「保存」才落盘（非自动保存），并提供「撤销」回退到原始值；底部在 `dirty` 时浮出保存条。
- **`saveSettings` 回滚**：store 中 `saveSettings` 先乐观更新本地 settings，调用后端 `settings_set` 失败时回滚到修改前的值并提示错误，避免 UI 与后端不一致。
- **签到页账号列表**：Checkin 页展示候选账号表格（名称/UserID、JWT 状态徽标、今日签到状态、积分），支持全部/分组/手动勾选三种范围与跳过规则；顶部固定「请勿一天内多次签到」防封警告。
- **`startCheckin` 重置**：发起签到前先重置 checkin 状态（`active:true, total:0, index:0, results:[], done:null`），避免显示上一次的进度残留。
- **日志页**：Logs 页支持「复制代理日志」与「复制查询日志」（写入剪贴板）；实时代理输出采用最新置顶（新行 `unshift` 到数组头部），最多保留 200 行。
- **Modal 组件**：弹窗打开时监听 `Escape` 键关闭，并锁定 `body` 滚动（`overflow:hidden`）；关闭时还原，防止背景滚动穿透。
- **代理日志竞态修复（v2.1）**：`ProxyLogsTab` 的 `showDetail` 使用 `useRef` 递增请求 ID，关闭弹窗时递增使正在进行的 API 请求失效，防止异步返回后重新打开已关闭的弹窗。
- **暗色模式图表（v2.1）**：Dashboard 和 Credits 页面的 Recharts 图表通过 `useIsDark()` hook 动态适配暗色模式，切换主题时图表颜色实时更新。
- **积分趋势三线图（v2.1）**：Credits 页面展示三条折线（积分总数/获得积分/消耗积分），数据来源于每日积分快照 `credits_daily.json`。

## 3. 数据模型

统一存于 `%APPDATA%\TraeWorkAssistant\`，分三个子目录：

| 文件 | 来源 | 说明 |
|---|---|---|
| `conf/app_settings.json` | v1.0 新增 | 应用设置 |
| `data/checkin_accounts.json` | 沿用原格式 | 账号 + JWT，原脚本与 Python 核心共用 |
| `data/device_map.json` | 沿用原格式 | user_id → 虚拟设备身份 |
| `data/groups.json` | v1.0 新增 | 分组定义 + membership（UserID→groupId） |
| `data/checkin_summary.json` | 沿用 | 最近一次签到结果 |
| `data/credits_history.json` | v1.0 新增 | 积分历史（看板绘图） |
| `data/account_cooldowns.json` | v2.0 新增 | 签到错误冷却状态（error_type + cooldown_until） |
| `data/remaining_credits.json` | v2.0 新增 | 各账号剩余积分缓存 |
| `data/api_pool.json` | v2.0 新增 | API 账号池状态（选中账号、轮转计数） |
| `data/credits_daily.json` | v2.1 新增 | 每日积分快照（total / earned / consumed，三线趋势图数据源） |
| `data/certs/` | 沿用 | 自签 CA |
| `data/profiles/<user_id>/` | 沿用 | 各账号 TW 登录态备份 |
| `logs/` | v1.0 新增 | proxy.log / checkin.log / switcher.log / api 请求日志 / 代理请求日志 |

账号唯一主键：`UserID`（JWT payload `data.id`，16 位数字）。分组信息独立存储，不污染原 JSON。

### 3.1 签到错误冷却状态机（v2.0 新增）

按 HTTP 状态码和响应体分类签到错误，每种类型对应不同冷却策略：

| 错误类型 | 触发条件 | 冷却时长 | 账号池处理 |
|---------|---------|---------|-----------|
| `PlanLimit` | 响应体含 `code:1005` | 12 小时 | 换号重试 |
| `SoftRate` | HTTP 429 | 60 秒 | 换号重试 |
| `SessionDead` | HTTP 401 | 永久（需重登） | 换号重试 |
| `NotFound` | HTTP 404 | 60 秒 | 换号重试 |
| `Server` | HTTP 5xx | 10 分钟（累计） | 换号重试 |
| `Client` | 其他 4xx | 10 分钟（累计） | 换号重试 |

冷却状态持久化到 `account_cooldowns.json`，含 `error_type` 和 `cooldown_until` 字段。签到成功且积分 > 0 时自动清除冷却（SessionDead 除外）。

### 3.2 API 账号池调度（v2.0 新增）

API 请求的智能选号策略：

1. 跳过禁用/冷却中/积分已过期/零积分账号
2. `creditsExpireAt` 非零者优先（有过期时间的账号）
3. 过期时间升序（最近过期的优先用，避免积分浪费）
4. 过期时间相同 → 积分降序
5. 单请求最多换号 3 次（`MaxRotate`）

状态持久化到 `api_pool.json`，重启后冷却状态保持。

### 3.3 每日积分快照（v2.1 新增）

每天计算一次积分快照并写入 `credits_daily.json`，供积分看板「近 7 日趋势」三线折线图使用：

| 字段 | 说明 | 计算方式 |
|------|------|---------|
| `date` | 本地日期 `YYYY-MM-DD` | — |
| `total` | 当日所有账号剩余积分之和 | 遍历各账号 `calc_remaining_credits` 求和 |
| `earned` | 当日获得积分 | 签到获得（`credits_history.json` 当天 delta 之和）+ 非签到获得（API 查询 `start_time` 在今日本地时间内且 `package_source_type != 9` 的积分包） |
| `consumed` | 当日消耗积分 | `|total - earned - 昨日total|`（取绝对值） |

非签到获得积分：通过 TRAE 积分查询接口的 `user_entitlement_pack_list` 中，`start_time` 落在今日本地时间范围内且 `package_source_type` 不为 9（签到来源）的积分包，累加 `credits_limit`。

### 3.4 Mutex 安全锁模式（v2.1 新增）

Rust 后端统一采用 `safe_lock()` 辅助函数替代 `Mutex::lock().unwrap()`，在锁被毒化（panic 导致）时通过 `unwrap_or_else(|e| e.into_inner())` 恢复内部数据继续运行，避免单个 panic 导致整个 API 服务崩溃。此模式应用于：

- `api_server/pool.rs` — 账号池状态读写
- `api_server/routes.rs` — 请求处理中 `active_uid` / `last_error` 读写
- `commands/api_server.rs` — API 服务运行时状态读写

同时，`Response::builder()...body().unwrap()` 统一替换为 `unwrap_or_else()` fallback，防止响应构建失败时 panic。

### 3.5 暗色模式图表适配（v2.1 新增）

前端使用共享 `useIsDark()` hook（`src/lib/useIsDark.ts`）监听 `document.documentElement` 的 `class` 属性变化，实时检测暗色模式切换。Recharts 图表（Dashboard 柱状图、Credits 折线图）根据 `isDark` 状态动态调整：

- 网格线/轴线颜色（light: `#e2e8f0` / dark: `#3f3f46`）
- 文本颜色（light: `#94a3b8` / dark: `#a1a1aa`）
- 柱状/折线颜色明度反转（暗色模式使用高明度色）
- Tooltip 背景与边框（light: 白底 / dark: `#18181b` 深色底）

## 4. 进程与子进程契约

### 4.1 `auto_checkin.py`（增强）
- 沿用：读 `checkin_accounts.json` → 逐账号 `status_check`+`signin` → 写 `checkin_summary.json`。
- **新增参数**（向后兼容，默认行为不变）：
  - `--json-stream`：每账号结果以单行 JSON（NDJSON）输出，便于前端逐条渲染。
  - `--accounts UID1,UID2`：仅签指定 UserID。
  - `--scope all|group:<id>`：执行范围（供分组/勾选场景）。
- NDJSON 示例：`{"type":"start","total":6}` / `{"type":"account","index":1,"user_id":"...","name":"...","status":"already|success|fail","delta":300,"elapsed":1.2}` / `{"type":"done","ok":5,"already":0,"failed":1}`
- **v2.0 增强**：签到失败时输出 `error_type`（PlanLimit/SoftRate/SessionDead/NotFound/Server/Client）和 `cooldown_until`（Unix 时间戳）；签到成功后自动查询剩余积分并写入 `remaining_credits.json`；积分 > 0 时自动清除冷却状态。

### 4.2 `device_proxy.py`
- 环境变量：`PROXY_PORT`（默认 8899）、`AUTO_CAPTURE_JWT`（默认 1）。
- 行为：透明 MITM；捕获 `trae.cn` / `trae.com.cn` 带 `Cloud-IDE-JWT` 的请求写回 `checkin_accounts.json`（按 UserID 匹配，exp 防降级）；仅对 `checkin_credits/claim` 改写设备头。
- **v2.0 增强**：新增 `mchost.guru` 到默认监听域名，MITM 解密 TRAE 对话流量；WebSocket 请求通过 `upgrade: websocket` 头检测并隧道转发（不记录消息内容）；代理请求日志按日期分割写入 `logs/` 目录。
- 日志：`proxy.log`（旧式实时日志）+ `logs/proxy_req_YYYY-MM-DD.log`（结构化日志，支持关键字/时间段查询），关键标记 `[JWT 自动更新]` / `[JWT 自动追加新账号]`，供 Rust 解析并 `emit` 事件。

### 4.3 `trae-switch-bridge.ps1`
- **非交互模式**：`-Action <Switch|Save|New|Reset|List|ResetDeviceIds>` + `-Json` + `-UserId <id>`，以 NDJSON 输出每步进度，供 Rust 转发渲染步骤条。
- **v2.0 增强**：新增 `ResetDeviceIds` 动作，执行 6 层设备标识重置（machineid 文件、storage.json telemetry/sqmId/aha.device.device_id、aha/TinyStorage、HKLM 注册表 MachineGuid、trae-webview 追踪数据）；多策略 TRAE 安装路径探测（运行进程、多盘符扫描、.lnk 快捷方式解析、注册表卸载项、LOCALAPPDATA）。
- 需管理员权限（重置 MachineGuid），Rust 侧以 runas 提权启动。
- **编码要求**：PowerShell 5 需 UTF-8 with BOM + CRLF 行尾（LF 无 BOM 会导致中文字符解析错误）。

### 4.4 API 网关（v2.0 新增）
- 内嵌 axum HTTP 服务，复用 Tauri tokio runtime，无需独立进程。
- 端点：`POST /v1/chat/completions`（对话，流式+非流式）、`GET /v1/models`（模型列表）、`GET /status`（账号池状态）、`GET /health`（健康检查）。
- Bearer API Key 鉴权（常量时间比较，防时序攻击）；API Key 留空时跳过鉴权。
- SSE 协议转换：SOLO 自定义事件（metadata/output/token_usage/done）→ OpenAI 标准 chunk 格式；通过 `spawn_blocking` 包装 ureq 同步请求实现流式转发。
- 账号池调度：积分过期最近者优先，最多 3 次换号重试；错误分类联动冷却状态机。
- 应用退出时自动停止 API 服务释放端口。

## 5. 风险与应对（摘要）

| 风险 | 等级 | 应对 |
|---|---|---|
| TW 升级导致接口/路径变化 | 高 | 核心逻辑留在可热更的 Python/PS；应用内检测版本并提示 |
| TRAE 启用证书固定 | 高 | 降级：改用登录授权获取可续期凭据（见设计文档 7.3） |
| 安全软件拦截 CA/代理 | 中 | 白名单指引 + 代码签名 |
| 多账号触发风控 | 中 | 免责声明 + 签到间隔随机抖动 + 不超个人使用并发 |
| JWT 明文存储 | 中 | 下版 DPAPI 加密；导出强制加密 |
| UAC 拒绝 | 低 | 明确提示 + 手动步骤 |
| Python 缺失 | 低 | 安装包内置 embeddable Python |
