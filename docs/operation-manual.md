# Trae Work 助手 · 运行手册

> 适用版本：v2.4.4 ｜ 平台：**仅 Windows 10 / 11**
> 本文档说明如何准备环境、启动开发、打包发布、日常运行与排错。
> v2.0 新增：本地 API 网关、代理日志、签到错误冷却状态机、积分过期感知调度、6 层设备标识重置。

---

## 0. 一句话流程

```powershell
git clone <repo> && cd trae-work-assistant
npm install                 # 装前端依赖
npm run tauri dev           # 开发模式（带 Rust 热重载）
# 发布：
npm run tauri build         # 产出 msi / nsis 安装包
```

---

## 1. 环境准备（一次性）

| 依赖 | 版本要求 | 用途 | 校验命令 |
| --- | --- | --- | --- |
| Windows | 10 / 11 | 运行与打包平台 | `winver` |
| Node.js | ≥ 18（建议 22） | 前端构建 / Tauri CLI | `node -v` |
| Rust | ≥ 1.77（stable，edition 2021） | Tauri 后端编译 | `rustc --version` |
| Python | ≥ 3.9 | 内置签到/代理脚本（运行时用系统 Python 或在 Windows 上内嵌） | `python --version` |
| WebView2 Runtime | Win11 自带 / Win10 需装 | 前端渲染内核 | 控制面板查看 |
| Visual Studio Build Tools | 含「C++ 桌面开发」工作负载 + Windows 10/11 SDK | Rust 编译原生依赖 | VS Installer 查看 |

### 1.1 Rust 工具链

```powershell
# 用 rustup 安装（若尚未安装）
winget install Rustlang.Rustup
rustup toolchain install stable
rustup default stable
rustup target add x86_64-pc-windows-msvc   # MSVC 目标（Windows 默认）
```

> 注意：本项目用 **MSVC** 工具链，需要安装 VS Build Tools 的「使用 C++ 的桌面开发」与对应 Windows SDK，否则 `cargo build` 会报链接错误。

### 1.2 WebView2

Win11 已内置；Win10 到 [Microsoft WebView2 下载页](https://developer.microsoft.com/microsoft-edge/webview2/) 安装「Evergreen Bootstrapper」。

### 1.3 Python（仅运行时）

开发/打包阶段不强制编译 Python。脚本 `src-python/` 会在构建时随 `tauri.conf.json` 的 `bundle.resources` 一并打包到安装目录的 `python/` 与 `ps/`。运行期由 Rust 探测系统解释器（`python` → `python3` → `py`），若资源目录内嵌 `python.exe` 则优先使用。

---

## 2. 获取代码与安装依赖

```powershell
git clone <your-repo-url> trae-work-assistant
cd trae-work-assistant
npm install
```

`npm install` 会安装两类依赖：
- **前端 / 构建**：React、Vite、Tailwind、Tauri CLI（`@tauri-apps/cli`）、TypeScript 等（见 `package.json`）。
- **Rust**：首次 `tauri dev` / `tauri build` 时由 Cargo 自动拉取（`src-tauri/Cargo.toml`：tauri 2、serde、chrono、rand、base64、plugin-shell/dialog/notification）。

---

## 3. 启动开发模式

```powershell
npm run tauri dev
```

该命令会：
1. 先执行 `beforeDevCommand`（`npm run dev`）启动 Vite 开发服务器（默认 `http://localhost:5173`）；
2. 再编译并启动 Rust 主进程，加载 WebView 指向 devUrl；
3. 监听源码变更：前端热更新，Rust 改动自动重新编译（首次较慢，后续增量快）。

开发期可直接在浏览器打开 `http://localhost:5173` 做纯前端调试（此时 Tauri API 不可用，涉及 `invoke` 的功能会报错，属正常）。

### 仅构建前端（不启 Rust）

```powershell
npm run dev        # 仅 Vite，5173
npm run build      # tsc 类型检查 + vite 生产打包到 dist/
npm run preview    # 预览 dist/ 产物
```

> 注意：`npm run build` 是纯前端构建（`tsc && vite build`），产物在 `dist/`，由 Tauri 在 `tauri build` 时消费（`frontendDist: ../dist`）。

---

## 4. 打包发布

```powershell
npm run tauri build
```

流程：
1. 执行 `beforeBuildCommand`（`npm run build`）产出 `dist/`；
2. 编译 Rust release；
3. 按 `bundle.targets`（`msi` + `nsis`）生成安装包；
4. 把 `src-python/` → `python/`、`src-ps/` → `ps/` 作为资源一起打包。

产物位置：

```
src-tauri/target/release/bundle/
├── msi/   *.msi
└── nsis/  *.exe
```

安装后即可从开始菜单启动「Trae Work 助手」，无需 Node / Rust 环境。

---

## 5. 日常运行（已安装用户视角）

1. 安装并启动一次 **Trae Work**，登录任意账号产生登录态；
2. 打开 **Trae Work 助手** → 顶栏提示「未安装 CA 证书」→ 点「一键安装证书」（弹 UAC）；
3. 点顶栏「启动代理」→ 日志页可见 `[代理] listening 127.0.0.1:8899`；
4. 在 Trae Work 中切换账号，代理自动捕获 JWT 写回本地；
5. 「账号管理」核对列表/分组；「一键签到」选择范围实时进度；「积分看板」看排行；
6. 「设置」可注册 Windows 计划任务，每天固定时间后台跑 Python 签到（无需开应用）。

### 5.1 账号管理

- **编辑账号**：在账号列表点击操作区的「编辑」按钮，可修改账号名称和/或粘贴新 JWT；保存后立即生效并刷新列表（更新 JWT 时会重新解析 UserID）。
- **查看 JWT**：点击「查看 JWT」打开弹窗，展示 UserID、JWT 剩余有效期、**过期时间**（按本地时区格式化）及 JWT 原文；点击「复制 JWT」可一键复制完整原文到剪贴板。弹窗支持按 `Esc` 关闭。
- **JWT 状态**：列表中 JWT 状态按剩余有效期着色（青绿=正常 / 琥珀=临期<24h / 珊瑚红=已过期）。

### 5.2 一键签到

- 签到页顶部以全部/指定分组/手动勾选三种范围展示**候选账号表格**（账号名、UserID、JWT 状态、今日签到状态、积分），勾选后点击「开始签到」实时查看逐账号进度。
- ⚠ **防封警告**：页面固定提示「请勿一天内多次签到」，重复签到可能导致账号被官方禁封；建议开启「跳过今日已签」。

### 5.3 运行日志

- 日志页分两栏：左侧「实时代理输出」（**最新置顶**，新日志滚动到顶部，最多 200 行），右侧「查询日志」（按类型/日期/关键字筛选）。
- 支持一键**复制**：代理输出与查询日志各有复制按钮，将内容写入系统剪贴板；查询日志还支持导出为 CSV。

### 5.4 设置

- 设置页采用「编辑 -> 保存」工作流：修改任意配置项后，顶部与底部出现「有未保存的更改」提示及「保存」「撤销」按钮；**点击「保存」才持久化**到 `app_settings.json`，未保存前可「撤销」回退到原始值。保存失败会自动回滚并提示错误。
- **系统设置**（v2.0 重命名）：菜单项从「设置」更名为「系统设置」，页面标题同步更新。

### 5.5 系统日志（v2.0 增强）

- 菜单项从「运行日志」更名为「系统日志」，包含两个页签：
  - **运行日志**：实时代理输出（最新置顶）+ 查询日志（按类型/日期/关键字筛选）。
  - **代理日志**（v2.0 新增）：结构化代理请求列表，支持：
    - 关键字查询（匹配 host/path/method）
    - 时间段过滤（起止时间选择）
    - 时间倒序排列
    - 多日志文件分割查询（按日期自动分割 `logs/proxy_req_YYYY-MM-DD.log`）
    - 点击单条日志打开详情弹窗（完整请求/响应头和体）
    - Method 列区分协议：HTTP GET（蓝）、HTTP POST（绿）、HTTP PUT/PATCH（橙）、HTTP DELETE（红）、WebSocket（紫）

### 5.6 API 服务（v2.0 新增）

- 侧栏「API 服务」页提供三卡片布局：接口配置、账号池选择、运行状态。
- **接口配置**：设置监听端口（默认 7864）、API Key（留空则跳过鉴权）、默认模型（如 `glm-5.2`）。
- **账号池选择**：勾选要纳入 API 服务的账号，持久化到 `api_pool.json`。
- **运行状态**：一键启停 API 服务，展示当前活跃账号、总请求数、账号池健康度。
- 启动后可通过 OpenAI 兼容协议调用：
  ```
  POST http://127.0.0.1:7864/v1/chat/completions
  GET  http://127.0.0.1:7864/v1/models
  GET  http://127.0.0.1:7864/status
  GET  http://127.0.0.1:7864/health
  ```
- 账号池自动轮转：积分过期最近者优先，单请求最多换号 3 次，错误自动联动冷却状态机。
- 应用退出时自动停止 API 服务释放端口。

### 5.7 签到错误冷却（v2.0 新增）

- 签到失败时按错误类型自动设置冷却：
  - `PlanLimit`（额度不足）：冷却 12 小时
  - `SoftRate`（429 限流）：冷却 60 秒
  - `SessionDead`（401 失效）：标记需重新登录，不自动重试
  - `NotFound`（404）：冷却 60 秒
  - `Server`/`Client`（5xx/其他 4xx）：累计错误，达阈值冷却 10 分钟
- 冷却中的账号在一键签到时自动跳过，账号列表展示冷却状态和剩余时间。
- 签到成功且积分 > 0 时自动清除冷却（SessionDead 除外）。
- 支持手动解冻：账号列表中每个冷却账号可点击「解冻」按钮。

### 5.8 设备标识重置（v2.0 新增）

- 系统设置页新增「执行 6 层重置」按钮，点击后自动 UAC 提权执行：
  1. `machineid` 文件 — 替换为新的 hex32 UUID
  2. `storage.json` — 替换 `telemetry.machineId` / `telemetry.sqmId` / `aha.device.device_id`
  3. `aha/TinyStorage` — 清除 `device_id`（强制重新注册）
  4. 注册表 `MachineGuid` — 替换
  5. `trae-webview` 追踪数据 — 清除 Cookies/Local Storage/Session Storage
  6. 删除 `has_device_id_updated_to_aha` 标记位
- 重置后 TRAE 启动时会用新设备 ID 注册，防止多账号被服务端关联。

### 5.9 剩余积分（v2.0 新增）

- 账号列表新增「剩余积分」列，展示各账号当前可用积分。
- 签到完成后自动刷新剩余积分。
- 支持手动刷新：点击账号列表的「刷新积分」按钮批量查询所有账号。

---

## 6. 数据目录与配置

所有数据仅存本机 `%APPDATA%\TraeWorkAssistant\`，分三个子目录：

```
%APPDATA%\TraeWorkAssistant\
├── conf/
│   └── app_settings.json       # 主题/端口/跳过策略/retry/log_retention_days…
├── data/
│   ├── checkin_accounts.json   # 账号 + JWT
│   ├── device_map.json         # 每账号独立伪设备 ID
│   ├── groups.json             # 分组定义
│   ├── credits_history.json    # 积分落盘
│   ├── checkin_summary.json    # 最近一次签到结果（今日已签徽章）
│   ├── account_cooldowns.json  # v2.0 签到错误冷却状态（error_type + cooldown_until）
│   ├── remaining_credits.json  # v2.0 各账号剩余积分缓存
│   ├── api_pool.json           # v2.0 API 账号池状态（选中账号、轮转计数）
│   ├── credits_daily.json      # v2.1 每日积分快照
│   ├── certs/                  # 自签 CA 证书
│   └── profiles/<user_id>/     # 切换登录态时备份/恢复的 TRAE Profile 快照
└── logs/
    ├── proxy.log               # 实时代理输出
    ├── checkin.log             # 签到日志
    ├── switcher.log            # 切换日志
    ├── proxy_req_YYYY-MM-DD.log # 结构化代理请求日志
    └── api_*.log               # API 服务请求日志
```

- 配置项在「设置」页修改，落盘到 `conf/app_settings.json`。
- 日志按 `log_retention_days`（设置项）在应用启动期自动清理过期行；设为 `0` 表示不清理。

---

## 7. 单测

Python 纯函数单测：

```powershell
python src-python/tests/test_auto_checkin.py
```

覆盖：JWT 解析、过期判定、`rand_digits` 同 seed 同输出（稳定设备 ID）。

Rust 端：需在 Windows 本机执行 `cargo test`（本仓库 CI/沙箱未覆盖 Rust 编译与端到端测试）。

---

## 8. 常见问题排错

| 现象 | 可能原因 | 处理 |
| --- | --- | --- |
| `cargo build` 链接失败 / 找不到 `link.exe` | 未装 VS Build Tools 或 C++ 工作负载 | 装「使用 C++ 的桌面开发」+ Windows SDK，确认 `x86_64-pc-windows-msvc` 目标 |
| 启动后白屏 / 前端报错 `invoke` 不存在 | 直接在浏览器打开了 5173，未走 Tauri 外壳 | 用 `npm run tauri dev` 启动，而非裸 `npm run dev` |
| 代理启动失败 / 捕获不到 JWT | 未安装 CA 证书，或 Trae 未走本地代理 | 「一键安装证书」（UAC）→ 启动代理 → 确认日志 listening |
| 提示「未检测到 Python」 | 系统未装 Python 或不在 PATH | 安装 Python ≥ 3.9 并加入 PATH；或确保资源目录内嵌 `python.exe` |
| 打包后运行报错缺脚本 | resources 未包含 | 确认 `tauri.conf.json` 的 `bundle.resources` 仍指向 `../src-python/` 与 `../src-ps/` |
| 安装包体积大 | 含 WebView2 引导 / 调试符号 | 属正常；发布用 release 产物即可 |
| 开启代理后 GitHub / Google 打不开（`ERR_TUNNEL_CONNECTION_FAILED`），baidu / qq 正常 | 系统代理被改写为 `127.0.0.1:8899`，覆盖了 VPN 接管点；非 Trae 域名直连绕过 VPN | v2.4.3 已修复：启动时自动捕获已有系统代理注入 `UPSTREAM_PROXY` 并串联转发。**Python 侧改动需 `npm run tauri build` 重新打包才在正式版生效** |
| 停止代理后 VPN 失效 | 旧版 `proxy_stop` 只把 `ProxyEnable` 置 0，未还原原 `ProxyServer` | v2.4.3 已改为原样还原启动前的 `ProxyEnable`/`ProxyServer`/`ProxyOverride` |
| 计划任务查询输出乱码 | `schtasks` 中文输出为 GBK，被按 UTF-8 解读 | v2.4.3 统一走 `misc.rs::run_schtasks()`（前置 `chcp 65001`）；新增 schtasks 调用勿裸调 `Command` |
| 注册计划任务报 Access Denied | 旧版使用 `/RL HIGHEST` 强制最高权限 | v2.4.3 已移除该参数，任务以当前用户身份运行 |

---

## 9. 约束与说明

- **仅 Windows 验证**：macOS / Linux 虽可编译 Tauri，但本项目代理证书与 MachineGuid 流程仅在 Windows 验证。
- **本地优先**：不连接任何自有服务器，账号/JWT/积分均留本机。
- **邀请链接**：固化在 Rust `INVITE_LINK` 常量（带 utm 参数），前端展示与复制均取自该常量。
- **积分看板**：已完整实现「积分总额 / 账号数 / 平均积分 / 今日新增积分」统计卡，并提供「近 7 日积分趋势」折线图与「账号积分排行」柱状图 + 明细列表；数据来自 `credits_history.json`（由签到脚本实时落盘，自动裁剪 90 天）。邀请入口已收敛到左侧菜单底部「邀请得 5000 积分」。

---

## 10. 命令速查

| 命令 | 作用 |
| --- | --- |
| `npm install` | 安装依赖 |
| `npm run dev` | 仅前端 Vite（5173） |
| `npm run build` | 前端类型检查 + 生产打包（`dist/`） |
| `npm run preview` | 预览 `dist/` |
| `npm run tauri dev` | 开发模式（前端 + Rust 热重载） |
| `npm run tauri build` | 打包 msi / nsis 安装包 |
| `python src-python/tests/test_auto_checkin.py` | Python 单测 |
