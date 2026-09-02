# AGENT.md — Trae Work Assistant v2.4.4

> 项目级别速查手册。给后续会话（人或 AI）秒接上下文用。任何会改契约的提交请同步更新本文档。

## 1. 一句话

Windows 桌面端多账号签到 + 登录态切换 + 设备隔离 + API 网关工具。**所有数据仅存在 `%APPDATA%\TraeWorkAssistant\`，零外部网络**。

## 2. Quick Start

```powershell
# 仅 Windows，需要 Node 18+ / Rust stable (MSVC) / VS Build Tools C++ 工作负载 / WebView2
cd trae-work-assistant
npm install
npm run tauri dev          # 开发模式（Tauri WebView 加载 Vite 5173）
npm run tauri build        # 打包 MSI + NSIS 到 src-tauri/target/release/bundle/
```

测试：

```powershell
python src-python/tests/test_auto_checkin.py   # Python 纯函数单测
cargo test                                    # Rust 单测（需先装工具链）
```

## 3. 技术栈

| 层 | 技术 |
|---|---|
| 外壳 | Tauri 2.x (Rust 1.75+ MSVC) |
| 前端 | React 18 + TypeScript 5 + Vite 5 + Tailwind 3 + Zustand 4 + Recharts 2 + lucide-react |
| 后端 | Rust (serde / chrono / axum / ureq / tauri-plugin-{shell,dialog,notification}) |
| 辅助 | Python 3.9+（仅标准库 + `cryptography`）+ PowerShell 5.1+（系统自带） |

## 4. 目录地图

```
trae-work-assistant/
├── AGENT.md                      # 本文件（项目速查）
├── README.md                     # 用户文档
├── package.json / vite.config.ts / tsconfig.json / tailwind.config.js / postcss.config.js / index.html
├── docs/                         # 设计/技术/API/用户手册/operation-manual
├── src/                          # 前端
│   ├── App.tsx                   # 外壳（TitleBar + Sidebar + TopBar + 页面切换 + Toaster）
│   ├── store.ts                  # Zustand 单一真相（init / 刷新 / checkin/switch/saveLogin 事件归约）
│   ├── types.ts                  # 与 Rust DTO 对齐（snake_case）
│   ├── lib/tauri.ts              # invoke 封装 + 事件订阅（setupListeners）
│   ├── components/               # TitleBar/Sidebar/TopBar/Toaster/PageHeader/SetupGuide/ui
│   └── pages/                    # Dashboard / Accounts / Checkin / Credits / Logs / ApiService / Settings
├── src-tauri/
│   ├── tauri.conf.json           # 无装饰窗 / bundle.resources = ../src-python/ + ../src-ps/
│   └── src/
│       ├── main.rs               # 注册全部命令
│       ├── state.rs              # AppState（%APPDATA%\TraeWorkAssistant + python_dir）
│       ├── models.rs             # DTO（含 CheckinSummary.time 字段）
│       ├── fs_utils.rs           # 原子 read_json / write_json / mask / 时间辅助
│       ├── jwt.rs                # parse() + status_of() + refresh() + oauth_parse()
│       ├── python.rs             # spawn_script（注入 TRAEDATA_DIR）
│       ├── api_server/           # API 网关模块
│       │   ├── mod.rs            # 常量 + 路由注册
│       │   ├── server.rs         # axum 服务器启停
│       │   ├── routes.rs         # OpenAI 兼容路由（SSE 流式 + 非流式）
│       │   ├── pool.rs           # 账号池调度（积分感知 + 冷却状态机 + 账号轮换）
│       │   ├── sse.rs            # SSE 协议转换（SOLO → OpenAI chunk）
│       │   ├── auth.rs           # Bearer Token 鉴权
│       │   └── api_logger.rs     # API 请求日志
│       └── commands/             # env / cert / accounts / checkin / proxy / switch / misc / profile / api_server / oauth
├── src-python/
│   ├── device_proxy.py           # MITM 代理（env TRAEDATA_DIR、--gen-ca）
│   ├── auto_checkin.py           # 批量签到（--json-stream / --accounts / --scope）
│   ├── requirements.txt          # cryptography
│   └── tests/test_auto_checkin.py
└── src-ps/trae-switch-bridge.ps1 # 非交互切换桥 + NDJSON 步骤输出
```

## 5. Tauri 命令契约

> **调用约定**：invoke 的**顶层参数名**跟随 Rust 函数签名（驼峰不替换，参数名直接匹配）。**嵌套对象**（`opts` / `patch`）的字段名保持 **snake_case**（Tauri 默认 serde 字段名，不做 camelCase 转换）。

| 模块 | 命令 | 说明 |
|---|---|---|
| 环境 | `env_check` → `EnvStatus` | `installed/running/version/path` |
| 证书 | `cert_status` / `cert_install` | 安装走 UAC `certutil -addstore -f Root` |
| 代理 | `proxy_start(port)` / `proxy_stop()` / `proxy_status()` | ProxyStatus：`running/port/captured/started_at` |
| 账号 | `accounts_list` → `AccountView[]` | 聚合 JWT / 分组 / 设备 / 积分 / 今日 |
| 账号 | `account_add_manual(name, jwt, groupId?)` | 解析 JWT → userId 入库 |
| 账号 | `account_delete(userId, deleteProfile)` | 同时清分组；`deleteProfile=true` 删 profiles/<uid> |
| 账号 | `account_oauth_add` → 实际为 `oauth_login(callback_url, account_name?, group_id?)` | 从 OAuth 回调 URL 解析 token + userInfo |
| OAuth | `oauth_get_login_url()` → `{ url }` | 构造 Trae 登录 URL |
| OAuth | `oauth_parse_callback(callback_url)` → `{ user_id, ... }` | 解析回调 URL 中的 token |
| 分组 | `groups_list` / `group_create` / `group_update` / `group_delete` / `group_move` | 删除分组时账号回落「未分组」 |
| 签到 | `checkin_start(opts)` → NDJSON 事件 | `opts: { scope, user_ids?, skip_checked_in, skip_expired }` |
| 切换 | `switch_account(userId)` | 调 `trae-switch-bridge.ps1 -Action Switch` |
| 保存 | `save_current_login(userId)` | 调 `trae-switch-bridge.ps1 -Action SaveCurrentLogin` |
| 快照 | `profile_list` → `ProfileInfo[]` | 列出 data/profiles/ 下所有快照 |
| 快照 | `profile_backup(userId)` / `profile_restore(userId)` / `profile_delete(slot)` | 手动备份/恢复/删除 |
| 设备 | `device_reset(userId)` | 删 `device_map.json[ uid ]` |
| JWT | `jwt_parse(jwt)` / `refresh_jwt(userId)` | 解析 / 自动刷新（需 refresh_token） |
| API | `api_server_start(port)` / `api_server_stop()` / `api_server_status()` | API 网关启停 |
| API | `pool_list` / `pool_set` / `pool_status` | 账号池管理 |
| API | `api_debug_toggle` / `api_debug_status` | API 请求日志开关 |
| 日志 | `logs_query({ opts: { log_type, date, keyword, limit } })` → `LogLine[]` | `split_time` 会 strip BOM 前缀 |
| 设置 | `settings_get()` / `settings_set(patch: Settings)` | Settings 全部 snake_case |
| 计划 | `task_register(time)` / `task_status()` / `task_unregister()` | `schtasks` 注册每日签到 |

## 6. Tauri 事件（Rust → 前端）

| 事件 | payload |
|---|---|
| `proxy-log` | `string`（代理 stdout 逐行） |
| `account-captured` | `string`（新捕获的 userId） |
| `checkin-progress` | `{type:'start',total}` / `{type:'account',...}` / `{type:'done',ok,already,failed}` |
| `switch-progress` | `string`（PowerShell NDJSON 单行） |
| `switch-done` | `{ success: boolean, raw: string }` |
| `save-login-progress` | `string`（PowerShell NDJSON 单行） |
| `save-login-done` | `{ success: boolean, raw: string }` |

## 7. 数据文件

```
%APPDATA%\TraeWorkAssistant\
├── conf/
│   └── app_settings.json         # Settings 全字段（snake_case）
├── data/
│   ├── checkin_accounts.json     # { accounts: [{name, UserID, jwt, refresh_token?, added_at}] }
│   ├── device_map.json           # { <userId>: { device_id, market_user_id, session_id } }
│   ├── groups.json               # { groups: [...], membership: {<uid>:<gid>} }
│   ├── credits_history.json      # { records: [{date,user_id,credits,delta}] }
│   ├── credits_daily.json        # 每日积分快照
│   ├── remaining_credits.json    # 各账号剩余积分缓存
│   ├── account_cooldowns.json    # 签到错误冷却状态（error_type + cooldown_until）
│   ├── api_pool.json             # API 账号池配置 + 状态
│   └── profiles/                 # 登录态快照
│       ├── current_account.txt   # 当前活跃账号 ID
│       └── <user_id>/            # 精准备份的 9 类核心文件
└── logs/                        # proxy / checkin / switcher / api / proxy-requests 日志
```

**写入约定**：`fs_utils::write_json` 用 `tmp + rename` 原子替换，避免断电损坏。

## 8. PowerShell 切换桥约定

- **非交互模式**：不需要 `#Requires RunAsAdministrator`，普通用户即可运行。
- `-Json` 时输出 NDJSON 单行 `{"stage":"...","status":"...","message":"...","time":"..."}`。
- 入口目录：`$env:APPDATA\TRAE SOLO CN` + `$env:APPDATA\TraeWorkAssistant\data\profiles`。
- **Action 参数**：`Switch` / `SaveCurrentLogin` / `ResetMachineId` / `ResetDeviceIds` / `BackupCurrent` / `RestoreOnly`。
- **精准备份**：仅复制 9 类核心登录文件（storage.json / state.vscdb / machineid / aha / Network 等），非全量镜像。
- **Switch 流程**：预检查目标快照 → 关闭 Trae Work → 保存当前到 last + 当前账号槽位 → 恢复目标 → 启动。
- **SaveCurrentLogin 流程**：关闭 Trae Work → 精准备份到 userId 槽位 → 启动。
- storage.json 路径：`User\globalStorage\storage.json`，键名用点号访问（`$storage.'telemetry.machineId'`）。

## 9. Python 约定

- **数据目录**：通过 `os.environ["TRAEDATA_DIR"]` 注入（Rust `spawn_script` 负责），缺省回退到脚本所在目录。
- **NDJSON**：`--json-stream` 输出 `{"type":"start"|"account"|"done",...}` 单行 JSON。
- **稳定设备 ID**：`device_map.json` 缺条目时由 `rand_digits(n, seed=user_id)` 派生。
- **docstring**：包含 Windows 路径时**必须用 raw 字符串 `r"""..."""`**。
- **上游代理链（v2.4.3）**：`device_proxy.py` 读取 `UPSTREAM_PROXY`（可选 `UPSTREAM_PROXY_USER` / `UPSTREAM_PROXY_PASS`），支持 `http://host:port` 与 `socks5://host:port` 两种形态。**非 Trae 域名**的 CONNECT 隧道（`tunnel_raw`）与明文 HTTP 转发优先经上游出站，上游不可用时回退直连；Trae 域名仍走本地 MITM 解密以捕获 JWT。

## 9.1 代理生命周期约定（v2.4.3）

- `proxy_start` **先**通过 `get_existing_win_proxy()` 读取当前系统代理（即用户的 VPN），作为 `UPSTREAM_PROXY` 注入 Python 进程，**再**用 `set_win_proxy` 改写为 `127.0.0.1:<port>`。顺序不可颠倒，否则会把自己当成上游造成死循环。
- `proxy_stop` 与看门狗**原样还原**启动前捕获的 `ProxyEnable` / `ProxyServer` / `ProxyOverride`，而非简单置 0，避免破坏 VPN 设置。
- `tunnel_raw` **必须**先回 `HTTP/1.1 200 Connection Established` 客户端才会发起 TLS 握手；连接上游失败时回 `502 Bad Gateway`，不可静默返回。

## 10. 前端约定

- **store 单例**：`useAppStore` 聚合所有状态；`init()` 在 `App.tsx` `useEffect` 启动一次。
- **样式**：Tailwind 3 + `darkMode:'class'`；amber 色系为视觉强调色。
- **snake_case**：前端类型定义（`types.ts`）的字段名与 Rust DTO 完全一致。
- **路由**：极简 `useState`，不引 react-router。
- **Modal**：不支持 `window.confirm()`，使用自定义 `Modal` 组件（支持 `size="lg"|"xl"`）。
- **按钮反馈**：所有异步按钮动作使用 `withMinDelay(promise, 1000)` 确保最少 1 秒 loading。

## 11. 常用任务 SOP

| 任务 | 路径 |
|---|---|
| 新增账号 | Accounts 页 → OAuth 登录 或 手动粘贴 JWT → 选分组 → 入库 |
| 保存登录态 | Accounts 行 → Save 图标 → `save_current_login(userId)` |
| 切换账号 | Accounts 行 → LogIn 图标 → `switch_account(userId)` |
| 快照管理 | Accounts 页 → 快照管理按钮 → 查看/备份/恢复/删除 |
| 重置设备 ID | Accounts 行 → RotateCcw 图标 → `device_reset(userId)` |
| 注册定时签到 | Settings 页 → 输入 `HH:MM` → 注册任务 |
| API 服务 | ApiService 页 → 配置端口/API Key → 选账号池 → 启动 |

## 12. 安全与合规

- **零外发**：不连接任何自有后端。
- **CA 证书**：仅本地回环 `127.0.0.1:8899`，自签根 CA 需 UAC 安装。
- **UAC**：仅在 `cert_install` 提权，切换桥已改为普通用户可运行。
- **API Key**：留空时跳过 Bearer Token 鉴权；配置时在前端掩码显示（前 4 + 后 4 + ****）。
- **API 网关**：v2.0 已实现本地 API 网关（axum + ureq），上游 `trae-api-cn.mchost.guru`。

## 13. 禁止与红线（Do NOT）

- ❌ 修改 Rust 命令嵌套参数（如 `CheckinOpts`）的字段名 → 与 Python 子进程 serde 契约耦合。
- ❌ 把 Rust 命令顶层参数改为 camelCase → Tauri 用 Rust 函数签名原名匹配。
- ❌ 引入 React Router / Redux / 额外 UI 库 → 保持依赖最小。
- ❌ 提交 `.workbuddy/`、`dist/`、`node_modules/`、`src-tauri/target/`、`__pycache__/`、`data/`（已在 `.gitignore`）。
- ❌ 使用 `window.confirm()` → Tauri WebView 不支持，用自定义 Modal。
- ❌ 使用 `api.prevent_close()` → 会导致 Chromium 1412 错误。
- ❌ 用 `npm run dev` 直接跑 Vite → 白屏，必须 `npm run tauri dev`。

## 14. 已知约束

- 仅 Windows（代理证书安装 + MachineGuid 重置只在 Windows 验证）。
- PowerShell 切换桥需 Win10/11 自带 PowerShell 5.1+。
- `profiles_dir` 路径为 `data_dir.join("data").join("profiles")`，注意 `data/` 子目录。
- LLM API 上游必须设置 `NO_PROXY=*` 避免系统代理循环。
- 日志文件首行可能有 BOM 前缀（PowerShell 5.1 `-Encoding UTF8`），`split_time` 已处理。
- JWT 默认 13 天过期；带 refresh_token 的账号可自动续期。
- **`schtasks` 中文输出是 GBK**，直接 `String::from_utf8_lossy` 会乱码。统一走 `misc.rs::run_schtasks()`（前置 `chcp 65001`），**不要**再裸调 `Command::new("schtasks")`。
- **计划任务不加 `/RL HIGHEST`**：签到脚本只读写 `%APPDATA%` 并运行 Python，加了会让普通用户注册失败（Access Denied）。
- **错误文案不重复加前缀**：Rust 端返回纯错误描述，`查询失败：` / `注册失败：` 等前缀由前端 `Settings.tsx` 统一拼接。
- **`src-python/` 会打包进 `resources/python/`**：Python 侧改动在正式版必须 `npm run tauri build` 重新打包才生效；`npm run tauri dev` 直读源码，重启对应功能即生效。
