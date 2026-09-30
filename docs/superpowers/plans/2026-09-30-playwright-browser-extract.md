# 内置 Playwright 浏览器提取 JWT 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 移除账号管理中的本地浏览器检测与本地配置复用，改用内置 Playwright Chromium 浏览器及每次独占的临时 Profile，强制用户在纯净窗口登录以可靠截获 JWT。

**Architecture:** 
1. 后端重构 `commands/browser_extract.rs`：删除 Edge/Chrome/Brave 候选路径与 `detect_browsers` 命令；实现 `find_builtin_browser`（支持自定义路径、Tauri 资源目录 `resources/browser`、exe 相对路径与开发期本地 Playwright 目录兜底）；启动时生成独立的 `temp_profile`（`std::env::temp_dir()/trae_extract_<uuid>`），退出时彻底清理该临时目录。
2. 前端重构 `Accounts.tsx` 与 `tauri.ts`：移除浏览器类型选择下拉框、本地配置勾选框和 `detect` 逻辑，精简启动参数仅保留 `groupId`，更新操作说明。
3. 资源与打包配置：在 `tauri.conf.json`、`.gitignore` 与 `scripts/package_portable.py` 中增加 `resources/browser` 支持。

**Tech Stack:** Rust (Tauri 2, chromiumoxide 0.7, tokio, uuid), React 18, TypeScript, Tailwind CSS.

---

### Task 1: 资源配置与环境准备

**Files:**
- Create: `resources/browser/.gitkeep`
- Modify: `.gitignore`
- Modify: `src-tauri/tauri.conf.json:35-39`
- Modify: `scripts/package_portable.py:8-10`

- [ ] **Step 1: 创建 resources/browser 目录并保持 git 跟踪**

创建 `resources/browser/.gitkeep` 文件以保持目录结构。

- [ ] **Step 2: 更新 .gitignore**

在 `.gitignore` 末尾增加忽略 `resources/browser/` 内大型二进制的规则：
```gitignore
# Playwright Chromium binaries (bundled during packaging, excluded from git)
resources/browser/*
!resources/browser/.gitkeep
```

- [ ] **Step 3: 更新 tauri.conf.json 资源映射**

在 `src-tauri/tauri.conf.json` 的 `bundle.resources` 中追加 `"../resources/browser/": "browser/"`：
```json
    "resources": {
      "../src-python/": "python/",
      "../src-ps/": "ps/",
      "../resources/browser/": "browser/"
    },
```

- [ ] **Step 4: 验证打包脚本 package_portable.py 兼容性**

检查 `scripts/package_portable.py`，确认其自动遍历 `conf["bundle"]["resources"]` 能够正确处理 `../resources/browser/`。

- [ ] **Step 5: 验证配置格式**

运行：`npm run tauri build -- --help` 或运行检查命令确保 json 语法正确。

---

### Task 2: 后端浏览器定位与临时 Profile 生命周期实现

**Files:**
- Modify: `src-tauri/src/commands/browser_extract.rs`
- Modify: `src-tauri/src/main.rs:109-113`
- Test: `src-tauri/src/commands/browser_extract.rs` (内置测试模块)

- [ ] **Step 1: 编写 find_builtin_browser 与路径探测的单元测试**

在 `src-tauri/src/commands/browser_extract.rs` 现有的 `mod tests` 中增加测试用例：
```rust
#[test]
fn test_find_in_playwright_dir_scanner() {
    // 验证扫描本地 ms-playwright 逻辑（若本机存在，能找到最新版本的 chrome.exe）
    let found = scan_ms_playwright_dir();
    if let Some(path) = found {
        assert!(path.is_file());
        assert!(path.to_string_lossy().contains("chrome.exe"));
    }
}
```

- [ ] **Step 2: 实现 scan_ms_playwright_dir 与 find_builtin_browser**

在 `src-tauri/src/commands/browser_extract.rs` 中：
1. 移除 `EDGE_CANDIDATES`, `CHROME_CANDIDATES`, `BRAVE_CANDIDATES`, `candidates_for_type`, `detect_browsers`, `DetectedBrowser`, `browser_extract_detect`。
2. 实现 `scan_ms_playwright_dir() -> Option<PathBuf>`：
   读取 `%LOCALAPPDATA%\ms-playwright`，读取所有 `chromium-*` 目录，按目录名倒序排序，返回第一个存在 `chrome-win64\chrome.exe` 或 `chrome-win\chrome.exe` 的完整路径。
3. 实现 `find_builtin_browser(app: &tauri::AppHandle, custom_path: Option<&str>) -> Option<PathBuf>`：
   - 若 `custom_path` 存在且非空，作为最高优先级；若有效则返回，否则返回 None。
   - 检查 `app.path().resource_dir()` 下的 `browser/chrome.exe`、`browser/chrome-win/chrome.exe`、`browser/chrome-win64/chrome.exe`。
   - 检查 `std::env::current_exe()` 同级 `resources/browser/` 下的对应路径。
   - 检查当前工作目录 `resources/browser/` 下的对应路径。
   - 兜底调用 `scan_ms_playwright_dir()`。

- [ ] **Step 3: 重构 BrowserExtractHandle 与独立临时 Profile 机制**

1. 在 `BrowserExtractHandle` 中增加字段：
   ```rust
   pub struct BrowserExtractHandle {
       pub child: Child,
       pub browser: Browser,
       pub tasks: Vec<tokio::task::JoinHandle<()>>,
       pub profile_dir: std::path::PathBuf,
   }
   ```
2. 更新 `kill_now` 实现：
   ```rust
   impl BrowserExtractHandle {
       pub fn kill_now(&mut self) {
           let _ = self.child.start_kill();
           for t in self.tasks.drain(..) {
               t.abort();
           }
           let profile = self.profile_dir.clone();
           tokio::spawn(async move {
               clean_temp_profile(&profile).await;
           });
       }
   }
   ```
3. 实现带延迟重试的安全清理函数 `clean_temp_profile(dir: &std::path::Path)`，处理 Windows 下 Chromium 进程刚退出时的文件句柄占用。

- [ ] **Step 4: 改造 browser_extract_start 与 browser_extract_stop 命令**

1. 修改 `browser_extract_start` 签名：
   ```rust
   #[tauri::command]
   pub async fn browser_extract_start(
       app: tauri::AppHandle,
       state: State<'_, AppState>,
       runtime: State<'_, Mutex<Option<BrowserExtractHandle>>>,
       group_id: Option<String>,
   ) -> Result<(), String>
   ```
2. 内部逻辑：
   - 移除 `browser_type`、`use_local_profile` 解析。
   - 浏览器定位调用 `find_builtin_browser(&app, settings.browser_path.as_deref())`。
   - 生成全新临时目录：`let temp_profile = std::env::temp_dir().join(format!("trae_extract_{}", uuid::Uuid::new_v4()));`
   - 参数设置 `--user-data-dir={}`，不再提供任何复用本地 session 的分支。
   - 启动后将 `profile_dir: temp_profile` 存入 `BrowserExtractHandle`。
3. 修改 `browser_extract_stop`：
   - 退出浏览器后调用 `clean_temp_profile` 清理临时目录。
4. 在流监听浏览器退出（`handler.next().await` 结束）时触发清理。

- [ ] **Step 5: 更新 main.rs 中的命令注册**

在 `src-tauri/src/main.rs` 的 `invoke_handler!` 列表中：
移除 `commands::browser_extract::browser_extract_detect,`。

- [ ] **Step 6: 运行 Rust 编译与测试**

运行：`cargo test --manifest-path src-tauri/Cargo.toml commands::browser_extract`
预期：所有单测编译并运行通过。

---

### Task 3: 前端接口与 Accounts 页面重构

**Files:**
- Modify: `src/types.ts:228-232`
- Modify: `src/lib/tauri.ts:157-162`
- Modify: `src/pages/Accounts.tsx:1370-1600`
- Modify: `src/pages/Settings.tsx:368-380`

- [ ] **Step 1: 更新 src/types.ts 与 src/lib/tauri.ts**

1. 在 `src/types.ts` 中移除 `DetectedBrowser` 接口。
2. 在 `src/lib/tauri.ts` 中：
   - 移除 `DetectedBrowser` 的导入。
   - 修改 `browserExtract` 接口：
     ```ts
     browserExtract: {
       start: (groupId?: string) => invoke('browser_extract_start', { groupId }),
       stop: () => invoke('browser_extract_stop'),
     },
     ```

- [ ] **Step 2: 重构 src/pages/Accounts.tsx**

1. 移除状态变量：`detectedBrowsers`, `selectedBrowserType`, `detecting`, `useLocalProfile`。
2. 移除弹窗打开时的 `api.browserExtract.detect` 调用与 `localStorage` 处理。
3. 简化 `start` 函数：直接调用 `api.browserExtract.start(gid || undefined)`。
4. 移除界面中的「提取浏览器」下拉框以及「使用本地浏览器配置」复选框。
5. 更新说明卡片文本：
   ```tsx
   <div className="rounded-lg border border-slate-200 bg-slate-50 p-3 text-xs leading-relaxed text-slate-600 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">
     1. 点击「启动提取浏览器」——应用会打开一个纯净的内置浏览器窗口；
     <br />
     2. 请在弹出的浏览器中完成账号登录，系统将在您登录成功瞬间自动捕获 JWT 并保存为账号；
     <br />
     3. 如需提取多个账号，在浏览器中退出登录并换号登录即可连续捕获；
     <br />
     4. 捕获完成后点击「完成并关闭」。适合 OAuth 授权页卡「认证中」时使用。
   </div>
   ```

- [ ] **Step 3: 更新 Settings.tsx 说明文案**

在 `src/pages/Settings.tsx` 中更新「提取浏览器路径」的 placeholder 与说明文本：
- `placeholder="留空则使用内置 Playwright 浏览器"`
- 说明文案：「浏览器提取 JWT」使用的浏览器 exe 路径；留空将使用内置 Playwright Chromium 浏览器。

- [ ] **Step 4: 运行前端类型检查与构建验证**

运行：`npm run build`
预期：TypeScript 编译无报错，Vite 打包成功。

---

### Task 4: 端到端功能验证与回归检查

**Files:**
- 全局验证

- [ ] **Step 1: 验证本地开发期 Playwright 自动兜底**

运行 `cargo test --manifest-path src-tauri/Cargo.toml` 验证后端逻辑无误。

- [ ] **Step 2: 界面联调与视觉检查**

检查 `src/pages/Accounts.tsx` 弹窗在亮色/暗色主题下的排版完整性，确认无残留的废弃选项与布局错位。

- [ ] **Step 3: 整体代码检查与 Git 提交**

运行 `git status` 确认修改范围仅涉及任务要求文件。
提交 commit：`feat(browser-extract): 改用内置 Playwright 浏览器并采用独立全新 Profile`
