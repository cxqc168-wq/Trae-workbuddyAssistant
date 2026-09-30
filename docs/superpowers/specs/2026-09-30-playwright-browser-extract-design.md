# 浏览器提取 JWT 改造（内置 Playwright 浏览器与独立临时会话）设计文档

- 日期：2026-09-30
- 状态：已批准
- 关联：`src-tauri/src/commands/browser_extract.rs`、`src/pages/Accounts.tsx`、`src/lib/tauri.ts`

## 1. 背景与问题定义

现有“浏览器提取”功能在实际使用中存在以下核心问题：
1. **旧会话导致 JWT 捕获失败**：当前实现复用了 `%APPDATA%\...\browser_profile`（或用户勾选的本地浏览器配置）。当浏览器已存在旧登录态时，打开 `https://www.trae.cn/` 会直接处于已登录状态，前端不再触发登录鉴权接口（未携带 `Authorization` 头），导致 CDP 永远无法抓取到有效的 JWT。
2. **本地浏览器检测维护复杂且不可靠**：检测用户本机的 Edge/Chrome/Brave 容易因浏览器后台进程常驻、版本兼容性或指纹策略问题导致 CDP 调试端口绑定失败。

**目标**：
- 彻底移除对用户本地 Edge/Chrome/Brave 的检测逻辑及前端对应选择控件。
- 改为使用软件内置的 Playwright Chromium 浏览器，开箱即用。
- 每次启动提取时，必须使用全新的独立临时 Profile，确保打开的必然是未登录状态，强制用户在弹出的新浏览器中完成登录，从而稳定捕获登录鉴权请求中的 JWT。

## 2. 详细设计

### 2.1 内置浏览器定位与回退策略 (`find_builtin_browser`)

移除 `EDGE_CANDIDATES`、`CHROME_CANDIDATES`、`BRAVE_CANDIDATES`、`detect_browsers` 及 `browser_extract_detect` 命令。
新增 `find_builtin_browser(app: &tauri::AppHandle) -> Option<PathBuf>`：

1. **生产包资源目录**：
   - 检查 `app.path().resource_dir()` 下的 `browser/chrome.exe`、`browser/chrome-win/chrome.exe` 或 `browser/chrome-win64/chrome.exe`。
2. **便携版/可执行文件同级目录**：
   - 检查当前 exe 同级目录下的 `resources/browser/` 及其子目录中的 `chrome.exe`。
3. **开发环境目录**：
   - 检查项目根目录下的 `resources/browser/`。
4. **开发期本地 Playwright 兜底**：
   - 若前三者均不存在，扫描系统目录 `%LOCALAPPDATA%\ms-playwright\`，按版本倒序匹配最新的 `chromium-*/chrome-win*/chrome.exe`。
5. **错误处理**：
   - 若均未找到，返回明确错误提示：“未找到软件内置浏览器组件，请确认安装包完整或存在 Playwright 浏览器”。

### 2.2 独立临时 Profile 与生命周期管理

1. **临时目录生成**：
   - 每次调用 `browser_extract_start`，使用系统临时目录生成独占路径：
     `std::env::temp_dir().join(format!("trae_extract_{}", uuid::Uuid::new_v4()))`
   - 启动浏览器时通过 `--user-data-dir` 传入该临时目录。
   - 不加载任何历史 Cookie 或 Session。
2. **清理机制**：
   - `BrowserExtractHandle` 增加 `profile_dir: PathBuf` 记录临时路径。
   - 在 `browser_extract_stop`、浏览器退出检测监听任务以及子进程错误清理中：
     - 先关闭 CDP / 终止子进程。
     - 等待进程释放文件句柄后，异步删除该临时目录（提供短延迟重试兜底应对 Windows 文件锁）。

### 2.3 后端命令与状态精简 (`browser_extract.rs`)

1. **参数精简**：
   `browser_extract_start` 移除 `browser_type` 与 `use_local_profile` 参数，仅保留 `group_id: Option<String>`。
2. **移除命令**：
   注销并移除 `browser_extract_detect` 命令。
3. **保留核心监听**：
   保留对 `api.trae.cn` 和 `api.trae.com.cn` 的 `Network.requestWillBeSent` 监听，拦截 `Authorization` 头（`Cloud-IDE-JWT` / `Bearer`），解析 JWT 保存账号并通过事件发送给前端。

### 2.4 前端 API 与界面重构 (`Accounts.tsx` & `tauri.ts`)

1. **API 契约 (`src/lib/tauri.ts`)**：
   ```ts
   browserExtract: {
     start: (groupId?: string) => invoke('browser_extract_start', { groupId }),
     stop: () => invoke('browser_extract_stop'),
   }
   ```
2. **界面变更 (`Accounts.tsx`)**：
   - 移除 `detectedBrowsers`、`selectedBrowserType`、`detecting`、`useLocalProfile` 状态。
   - 移除 `useEffect` 中对 `api.browserExtract.detect` 的调用及 `localStorage` 存取逻辑。
   - 移除「提取浏览器」下拉框与「使用本地浏览器配置」复选框。
   - 保留「新账号分组（可选）」、启动/重新启动按钮、运行中日志面板及捕获结果列表。
   - 步骤引导文本更新为提示用户在弹出的新窗口中完成登录。

### 2.5 打包与资源配置

1. **`src-tauri/tauri.conf.json`**：
   在 `bundle.resources` 中增加 `"../resources/browser/": "browser/"` 映射。
2. **`.gitignore`**：
   忽略 `resources/browser/` 中的大型二进制文件，保留 `.gitkeep`。
3. **打包脚本 `scripts/package_portable.py`**：
   确保 `resources/browser/` 复制到便携版发布目录中。

## 3. 测试与验证方案

1. **单测**：
   - 运行现有的 token 归一化与 URL 匹配测试：`cargo test --bin trae-work-assistant commands::browser_extract`。
2. **内置浏览器发现测试**：
   - 测试在缺少资源目录时能正确回退到 `%LOCALAPPDATA%\ms-playwright` 下的 Chromium。
3. **全新临时会话测试**：
   - 点击启动浏览器，验证弹出的窗口为干净未登录状态。
   - 在页面中执行登录，验证后台拦截到 JWT 并实时推送到前端列表。
   - 点击「完成并关闭」，验证浏览器进程正常退出，临时 profile 目录被安全清理。
4. **前端构建与类型检查**：
   - 执行 `npm run build` 确保无 TypeScript 类型错误与构建异常。
