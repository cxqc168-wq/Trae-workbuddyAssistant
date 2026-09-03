# 浏览器一键提取 JWT 登录 — 设计文档

- 日期：2026-09-03
- 状态：已批准
- 关联：OAuth 登录（`src-tauri/src/commands/oauth.rs`）、账号管理（`src/pages/Accounts.tsx`）

## 1. 背景与动机

现有 OAuth 登录流程（点击按钮 → 打开浏览器 → 官网授权 → 回调 17388 端口）存在卡死问题：
官网授权页在「认证中，正在验证身份」阶段会从浏览器侧探测 `127.0.0.1:17388`，
若用户使用带代理的指纹浏览器，探测请求经代理转发而失败；或 17388 端口被真正的
Trae 客户端占用（回调被 Trae 自己接走）。两种情况都会导致授权页永远卡在「认证中」。

替代路径：直接在浏览器中登录 trae.cn，从其发往 `api.trae.com.cn/cloudide/*` 的请求
`Authorization` 头中提取 JWT（支持 `Cloud-IDE-JWT <token>` / `Bearer <token>` / 裸
token 三种格式），跳过 OAuth 回调机制。

## 2. 已确认的关键决策

| 决策点 | 选择 |
|---|---|
| 集成形态 | 应用内一键提取（账户页新增入口，自动启动浏览器、拦截、保存） |
| 技术选型 | 纯 Rust CDP（chromiumoxide crate 驱动系统 Edge/Chrome），零 Python 依赖 |
| Profile 策略 | 单一持久 profile（存应用数据目录），登录态保留，JWT 过期后重提取免登录 |

多账号提取方式：提取到一个账号后，在浏览器内退出登录、换下一个账号登录，继续提取；
每次捕获都会实时出现在应用弹窗的捕获列表中。

## 3. 架构

```
账号管理页「浏览器提取」按钮
  → Rust 启动系统 Edge/Chrome（持久 profile，专用调试端口）
  → 用户在浏览器中登录 trae.cn
  → CDP Network.requestWillBeSent 拦截 trae 域名请求的 Authorization 头
  → token 归一（补 Cloud-IDE-JWT 前缀）→ jwt::parse 取 user_id
  → 已存在则更新 JWT，新账号则保存（自动调 GetUserInfo 取昵称，失败则 账号_<前8位>）
  → emit browser-extract-captured / browser-extract-progress 事件
  → 前端弹窗实时显示捕获状态与账号列表
  → 「完成并关闭」→ browser_extract_stop（关闭浏览器 + 清理 CDP 连接，登录态保留）
```

## 4. Rust 侧 — 新模块 `src-tauri/src/commands/browser_extract.rs`

### 4.1 新增依赖（Cargo.toml）

- `chromiumoxide = "0.7"`（tokio 原生 CDP 客户端，需 `futures` 配合）
- `futures = "0.3"`
- tokio features 增加 `time`、`process`

### 4.2 浏览器发现顺序

1. `settings.json` 的 `browser_path`（用户在设置页配置，可选）
2. Edge 标准路径：`C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe`、
   `C:\Program Files\Microsoft\Edge\Application\msedge.exe`
3. Chrome 标准路径：`C:\Program Files\Google\Chrome\Application\chrome.exe`、
   `C:\Program Files (x86)\Google\Chrome\Application\chrome.exe`

全部不存在 → 返回错误，提示用户在设置页配置浏览器路径。

### 4.3 Tauri 命令（状态管理模式仿照 OAuthCallbackHandle）

- `browser_extract_start`：
  - 端口管理：在 9222-9333 范围内找空闲端口启动 `--remote-debugging-port`，
    避免与用户自己开的调试端口冲突
  - 启动参数：`--user-data-dir=<app_data>/browser_profile`、
    `--no-first-run`、`--no-default-browser-check`、`--remote-debugging-port=<port>`
  - 启动后打开 `https://www.trae.cn/`（用户从此登录）
  - 通过 CDP 订阅 `Network.requestWillBeSent`，过滤 URL 含 `api.trae.com.cn/cloudide`
    的请求，提取 `Authorization` 头
  - 拦截到有效 token 后的处理：归一格式 → `jwt::parse` 取 user_id →
    复用 oauth.rs 的账号保存模式（存在则更新 jwt，不存在则新增 + GetUserInfo 昵称）→
    emit `browser-extract-captured` 事件（载荷：user_id、name、exp_hours、is_new）
  - 同一 user_id 的重复拦截去重：本次提取会话内每个 user_id 只捕获保存一次
    （首次有效捕获为准；用户在浏览器内退出换号会产生新 user_id，不受影响）
  - 监听浏览器进程退出（用户手动关闭浏览器）→ emit `browser-extract-progress`
    （type: exited）通知前端
- `browser_extract_stop`：优雅关闭浏览器进程 + 断开 CDP 连接（幂等）

### 4.4 事件定义

- `browser-extract-captured`：`{ user_id, name, exp_hours, is_new }`
  —— 每捕获一个账号发一次（去重后）
- `browser-extract-progress`：`{ type: 'started' | 'browser-path' | 'exited' | 'error',
  message: string }` —— 状态流（已启动/浏览器路径/用户关闭了浏览器/错误）

## 5. 前端 — Accounts.tsx

### 5.1 入口

PageHeader 新增「浏览器提取」按钮（与「OAuth 登录」并列，`Globe`/`ScanSearch` 图标）。

### 5.2 BrowserExtractModal

- 顶部：三步流程说明（启动浏览器 → 登录 → 自动捕获）
- 中部：实时状态日志（来自 progress 事件）
- 捕获列表：本次会话捕获的账号（名字 / user_id / 有效期 exp_hours /
  新增或更新徽标），实时追加
- 分组选择下拉：本次捕获的账号统一入组（可选，默认不分组）
- 底部按钮：
  - 「完成并关闭」→ 调 `browser_extract_stop` + 关弹窗 + 刷新账号列表
  - 弹窗直接关闭（X）→ 同样触发 stop（防泄漏浏览器进程）
- 关闭弹窗时若未显式 stop，onClose 路径兜底调用 stop

### 5.3 OAuth 弹窗提示

OAuth 登录弹窗第 2 步（等待授权页卡「认证中」的位置）增加提示文案：
「若长时间卡在『认证中』（常见于指纹浏览器代理或 17388 端口被占用），
可关闭并改用『浏览器提取』登录」。

## 6. 无 refresh_token 的说明与配套修改

浏览器提取的账号没有 refresh_token（网站流量中拿不到）：

- 账号列表「刷新 JWT」操作对此类账号的报错信息优化：
  「该账号无 refresh_token，无法自动刷新。请使用『浏览器提取』重新获取 JWT」
- JWT 过期后的重提取 = 一键操作（持久 profile 已保持登录态，打开即已登录）

## 7. 设置项

`settings.json` 新增可选字段 `browser_path: string`（留空 = 自动检测）。
Settings 页新增「提取浏览器路径」输入框 + 说明文案。
Settings Rust 结构体（`settings_get`/`settings_set`）同步扩展字段。

## 8. 错误处理

| 场景 | 处理 |
|---|---|
| 找不到浏览器 | start 返回错误，前端 toast 提示去设置页配置路径 |
| 调试端口被占用 | 逐个尝试范围内端口，全部失败才报错 |
| 用户直接关闭浏览器 | 后台任务检测进程退出，emit progress(exited)，弹窗显示「浏览器已关闭，可重新启动」 |
| CDP 连接失败 | 清理子进程并报错，不留孤儿进程 |
| stop 重复调用 | 幂等（handle 为 None 直接 Ok） |
| 拦截到的 token 无效/解析失败 | 跳过，不打扰用户（日志记录） |

## 9. 测试

- Rust 单测：
  - Authorization 头匹配与归一逻辑（三种格式 → 统一 Cloud-IDE-JWT 前缀）
  - user_id 去重逻辑
  - 浏览器发现顺序（路径存在性判定，用注入的路径列表测试）
- 验证链：`cargo check` + `cargo test` + `tsc --noEmit` + 实际运行完整流程
  （启动 → 登录 → 捕获 → 关闭 → 重开免登录验证持久 profile 生效）

## 10. 非目标（YAGNI）

- 不做每账号独立 profile 管理（单持久 profile + 浏览器内换号足够）
- 不做 Playwright/Python 集成
- 不做 WorkBuddy 侧的浏览器提取（WorkBuddy 已有自己的 OAuth 轮询流程）
- 不捕获/保存 refresh_token 之外的其他凭据
