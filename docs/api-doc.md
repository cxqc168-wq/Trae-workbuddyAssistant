# API 文档 — Tauri 前端 ↔ Rust 命令契约

> 前端通过 `invoke('command', args)` 调用。所有命令运行在 Rust 主线程（耗时操作放子线程并通过事件回传）。错误以 `string` 返回（空=成功），或以 `{ ok: boolean, error?: string }` 结构返回。

## 1. 环境检测

### `env_check()` → `EnvStatus`
- 返回：`{ installed: boolean, running: boolean, version: string|null, path: string|null }`
- 副作用：无。

### `open_trae_website()` → `void`
- 打开 `https://www.trae.cn`（未安装引导）。

## 2. CA 证书

### `cert_status()` → `{ installed: boolean }`
### `cert_install()` → `{ installed: boolean, error?: string }`
- 提权（UAC）将 `certs/ca.cer` 装入本地计算机受信任根证书颁发机构。

## 3. 代理服务

### `proxy_start(port: number)` → `{ ok: boolean, error?: string }`
- 启动 `device_proxy.py` 子进程（env: `PROXY_PORT`, `AUTO_CAPTURE_JWT=1`）。
- 事件 `proxy-log`（逐行）、`account-captured`（uid）。

### `proxy_stop()` → `{ ok: boolean }`
- 优雅退出（CTRL_BREAK）→ 超时强杀。

### `proxy_status()` → `{ running: boolean, port: number, captured: number, started_at: number|null }`

## 4. 账号与分组

### `accounts_list()` -> `Account[]`
- `Account`: `{ userId, name, groupId|null, jwt: string, jwtExpHours: number|null, jwtExpTimestamp: number|null, checkedToday: boolean|null, credits: number|null, deviceIdMasked: string|null, remainingCredits: number|null, cooldown: CooldownInfo|null }`
- `jwt` 为账号完整 JWT 原文（供前端「查看 JWT」弹窗展示与复制）；`jwtExpTimestamp` 为 JWT 过期 Unix 时间戳（秒），供前端格式化过期日期。
- `credits` 取 `credits_history.json` 中该账号**最新日期**的余额（非历史峰值），无记录时为 `null`。
- `remainingCredits`（v2.0 新增）：取 `remaining_credits.json` 中缓存的剩余积分，无记录时为 `null`。
- `cooldown`（v2.0 新增）：`{ errorType: string, cooldownUntil: number, reason: string }`，无冷却时为 `null`。

### `credits_history()` → `CreditRecord[]`
- 返回积分历史明细（按日期聚合给看板绘图）。
- `CreditRecord`: `{ date: string, user_id: string, credits: number, delta: number }`（`date` 为本地 `YYYY-MM-DD`；`credits`=当日余额，`delta`=当日新增）。
- 数据源 `credits_history.json` 由 `auto_checkin.py` 在签到时按日期**追加落盘**（自动裁剪到 90 天）。

### `account_add_manual(name, jwt, groupId?)` -> `{ ok, error? }`
### `account_update(userId, name?, jwt?)` -> `{ ok, error? }`
- 更新账号名称和/或 JWT。传 `jwt` 时重新解析 UserID 并同步写回（JWT 可能换了账号），同时刷新 `updated_at`；`name`/`jwt` 均可选，仅传需要修改的字段。
### `account_delete(userId, deleteProfile)` -> `{ ok, error? }`

### `groups_list()` → `Group[]`
- `Group`: `{ id, name, color, order, count }`

### `group_create(name, color)` → `{ id }`
### `group_update(id, name?, color?, order?)` → `{ ok }`
### `group_delete(id)` → `{ ok }`（账号回落未分组）
### `group_move(userId, groupId|null)` → `{ ok }`

## 5. 签到

### `checkin_start(opts)` → `{ ok, error? }`
- `opts`: `{ scope: "all"|"group:<id>"|"selected", userIds?: string[], skipCheckedIn: boolean, skipExpired: boolean }`
- 启动 `auto_checkin.py --json-stream --scope ... --accounts ...`，通过事件 `checkin-progress` 回传 NDJSON 行（`start`/`account`/`done`）。
- 完成事件 `checkin-done`：`{ ok, already, failed, total }`。

## 6. JWT 续期 / 切换 / 设备

### `jwt_parse(jwt)` -> `{ userId, expHours, expTimestamp, status: "ok"|"warn"|"expired"|"unknown" }`
- `expTimestamp` 为 JWT 过期 Unix 时间戳（秒）。
- `exp` 兼容整数与浮点数（依次尝试 `as_i64` / `as_f64` / 数字字符串解析）；`userId`（payload `data.id`）兼容字符串与整数。

### `switch_account(userId)` -> `{ ok, error? }`
- 调 PowerShell 切换器（非交互 `-Action Switch -UserId`）；事件 `switch-progress` 回传步骤。
### `device_reset(userId)` → `{ ok, error? }`
- 删除 `device_map.json` 该条目。

## 7. 日志 / 设置 / 邀请

### `logs_query(opts)` → `LogLine[]`
- `opts`: `{ type?: "proxy"|"checkin"|"switch"|"system"|"error", date?: "today", keyword?: string, limit?: number }`
- `LogLine`: `{ time, type, message }`

### `settings_get()` / `settings_set(patch)` → `AppSettings`
### `invite_link()` → `{ url }`（固化邀请链接）

## 8. Python 子进程 NDJSON 协议（签到）

`auto_checkin.py --json-stream` 输出：

```
{"type":"start","total":6}
{"type":"account","index":1,"user_id":"4487…","name":"清风杜宾","status":"already","credits":8200,"elapsed":0.31}
{"type":"account","index":2,"user_id":"1556…","name":"青衣网络","status":"success","delta":300,"elapsed":1.24}
{"type":"account","index":3,"user_id":"…","name":"…","status":"fail","code":1001,"message":"JWT 无效","error_type":"SessionDead","cooldown_until":null}
{"type":"account","index":4,"user_id":"…","name":"…","status":"fail","code":1005,"message":"额度不足","error_type":"PlanLimit","cooldown_until":1786700000}
{"type":"done","ok":5,"already":0,"failed":1}
```

`status` 取值：`already`(已签) / `success` / `fail`。前端据此渲染进度与颜色。

- 账号行附带 `credits`（签到后最新余额）与 `delta`（本次新增积分，仅 `success` 有值）。前端积分看板据此**实时刷新**余额与「今日新增」，无需等待 `done`。
- 每次签到，`auto_checkin.py` 会把 `{date, user_id, credits, delta}` 追加写入 `credits_history.json`（已签/失败也写一行，delta 为 0），供 `credits_history()` 命令与看板趋势图读取。
- **v2.0 新增**：`fail` 行附带 `error_type`（PlanLimit/SoftRate/SessionDead/NotFound/Server/Client）和 `cooldown_until`（Unix 时间戳，`null` 表示不冷却）。签到成功且积分 > 0 时自动清除该账号的冷却状态。

## 9. 数据 DTO 关系

- `UserID` 为所有关联主键；`Account.groupId` → `Group.id`；`Account.deviceIdMasked` 来自 `device_map.json`；`credits` 来自 `checkin_summary.json` / `credits_history.json`。
- `Account.remainingCredits` 来自 `remaining_credits.json`（v2.0 新增）。
- `Account.cooldown` 来自 `account_cooldowns.json`（v2.0 新增）。

---

## 10. API 服务（v2.0 新增）

### `api_server_start(port, api_key, default_model)` → `{ ok, error? }`
- 启动内嵌 axum HTTP 服务，监听指定端口（默认 7864）。
- `api_key` 留空时跳过 Bearer Token 鉴权。
- `default_model` 为默认对话模型（如 `deepseek-v4-flash`）。支持模型：`deepseek-v4-flash`、`deepseek-v4-pro`、`glm-5.2`、`glm-5.3`、`doubao-seed-2.1-pro`、`doubao-seed-2.1-turbo`、`minimax-m3`、`kimi-k2.7-code` 等（大小写不敏感）。
- 应用退出时自动停止服务释放端口。

### `api_server_stop()` → `{ ok }`
- 停止 API 服务。

### `api_server_status()` → `{ running: boolean, port: number, api_key: string, default_model: string, pool_size: number }`
- 返回 API 服务运行状态和账号池信息。

### `api_pool_list()` → `PoolAccount[]`
- 返回可选入账号池的账号列表。
- `PoolAccount`: `{ userId, name, selected: boolean, remainingCredits: number|null, cooldown: CooldownInfo|null }`

### `api_pool_set(userIds)` → `{ ok }`
- 设置账号池选中的账号（持久化到 `api_pool.json`）。

### `api_pool_status()` → `{ accounts: PoolAccount[], activeAccount: string|null, totalRequests: number }`
- 返回账号池详细状态。

---

## 11. 代理日志（v2.0 新增）

### `proxy_logs_list(opts)` → `ProxyLogEntry[]`
- `opts`: `{ keyword?: string, start_time?: number, end_time?: number, limit?: number }`
- `ProxyLogEntry`: `{ timestamp, method, host, path, status_code, duration_ms, log_file, line_number }`
- 支持跨日志文件按时间倒序查询，`method` 区分 HTTP GET/POST/PUT/DELETE 和 WebSocket。

### `proxy_log_detail(log_file, line_number)` → `ProxyLogDetail`
- `ProxyLogDetail`: `{ timestamp, method, host, path, status_code, request_headers, response_headers, request_body, response_body, duration_ms }`
- 返回单条代理日志的完整详情。

---

## 12. 剩余积分（v2.0 新增）

### `fetch_remaining_credits(userId)` → `{ ok, remaining: number, error? }`
- 查询单个账号的剩余积分（从 TRAE 服务端实时获取）。
- 结果缓存到 `remaining_credits.json`。

### `refresh_remaining_credits()` → `{ ok, results: RemainingResult[] }`
- 批量刷新所有账号的剩余积分。
- `RemainingResult`: `{ userId, name, remaining: number, status: "ok"|"fail" }`

---

## 13. 设备标识重置（v2.0 新增）

### `switch_reset_device_ids()` → `{ ok, error? }`
- 执行 6 层设备标识重置（需管理员权限，自动 UAC 提权）：
  1. `machineid` 文件 — 替换为新的 hex32 UUID
  2. `storage.json` 中 `telemetry.machineId` / `telemetry.sqmId` — 替换
  3. `storage.json` 中 `aha.device.device_id` — 替换
  4. `aha/TinyStorage` 中 `device_id` — 清除
  5. 注册表 `HKLM:\SOFTWARE\Microsoft\Cryptography\MachineGuid` — 替换
  6. `trae-webview` 追踪数据 — 清除
- 删除 `has_device_id_updated_to_aha` 标记位，强制 TRAE 重新注册设备。

---

## 14. 每日积分快照（v2.1 新增）

### `credits_daily_list()` → `CreditsDailySnapshot[]`
- 返回每日积分快照列表（供积分看板三线趋势图使用）。
- `CreditsDailySnapshot`: `{ date: string, total: number, earned: number, consumed: number }`
  - `date`: 本地日期 `YYYY-MM-DD`
  - `total`: 当日所有账号剩余积分之和
  - `earned`: 当日获得积分（签到获得 + 非签到获得）
  - `consumed`: 当日消耗积分（`|total - earned - 昨日total|`）
- 数据源 `credits_daily.json`，每次刷新剩余积分时自动记录当天快照，保留 90 天。

---

## 15. 冷却状态管理（v2.1 新增）

### `cooldown_clear(userId)` → `Result<(), String>`
- 手动清除指定账号的冷却状态。
- 从 `account_cooldowns.json` 中移除该账号的冷却记录。

---

## 16. JWT 刷新（v2.1 新增）

### `refresh_jwt(userId)` → `Result<String, String>`
- 使用 `refresh_token` 调用 TRAE OAuth ExchangeToken API 刷新 JWT。
- 成功后原子写回新的 `accessToken` + `refresh_token` 到 `checkin_accounts.json`，返回新 JWT。
- 使用 `jwt_refresh_lock` 防止并发刷新，持锁后 double-check 文件防止重复刷新。
- 无 `refresh_token` 的账号返回错误（需手动重新捕获 JWT）。
