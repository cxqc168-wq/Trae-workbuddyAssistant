# 更新日志

本文件记录 Trae Work Assistant 的版本变更。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循语义化版本。

---

## [2.4.4] - 2026-08-16

维护版本：清理临时文档并同步版本号。

### 变更

- 删除临时问题分析报告 `docs/issue-analysis-2026-08-16.md`，其功能已由 `CHANGELOG.md` 与 `AGENT.md` 中的变更说明覆盖，避免重复维护。
- 版本号 2.4.3 → 2.4.4（`package.json` / `tauri.conf.json` / `Cargo.toml` / `Cargo.lock` 四处同步）。
- 同步更新 `README.md`、`AGENT.md`、`docs/user-manual.md`、`docs/tech-framework.md`、`docs/operation-manual.md` 中的版本标注，以及 `scripts/make_portable_zip.py` 的便携包文件名。

### 说明

- 本版本**无代码逻辑改动**，仅文档与版本号维护；v2.4.3 的代理/VPN 共存与定时任务修复保持有效。
- 若需重新打包安装包，仍须执行 `npm run tauri build`（Python 侧修复已随 v2.4.3 打包）。

---

## [2.4.3] - 2026-08-16

修复「开启本地代理后 GitHub / Google 打不开」与「定时签到注册·查询·取消无反应」两类问题。

### 修复

- **本地代理与 VPN 冲突导致外网无法访问**（`ERR_TUNNEL_CONNECTION_FAILED`）
  - 根因：`proxy_start` 把 Windows 系统代理**整体覆盖**为 `127.0.0.1:8899`，抹掉了 VPN（Clash / v2rayN 等本地 HTTP/SOCKS 代理）的接管点；而 `tunnel_raw()` 对非 Trae 域名使用 `socket.create_connection` **直连**上游，完全绕开 VPN，导致 GitHub / Google 被阻断，而 baidu / qq 等国内站点直连可达故始终正常。
  - 修复：引入**上游代理链式转发**。`proxy_start` 在改写系统代理**之前**先读取已有的系统代理配置，作为 `UPSTREAM_PROXY` 环境变量注入 Python 代理进程；`device_proxy.py` 新增 `_parse_upstream()` / `connect_via_upstream()`，支持 **HTTP CONNECT** 与 **SOCKS5**（含用户名密码认证）两类上游。`tunnel_raw()` 与明文 HTTP 转发路径对**非 Trae 域名**优先经上游（即 VPN）出站，上游不可用时自动回退直连。Trae 域名仍由本代理 MITM 解密以捕获 JWT。
- **CONNECT 隧道缺少握手应答**
  - `tunnel_raw()` 从未向客户端回送 `HTTP/1.1 200 Connection Established`，客户端因此永远不会发起 TLS 握手；上游不可达时也无任何应答，浏览器无限等待。现已补齐 `200` 握手，失败时回 `502 Bad Gateway`。
- **停止代理会破坏 VPN 设置**
  - `proxy_stop` 原先只是把 `ProxyEnable` 置 0。现改为**原样还原**启动前捕获的系统代理（含 `ProxyServer` 与 `ProxyOverride`），停止本地代理后 VPN 立即恢复可用。
- **计划任务查询结果中文乱码**
  - 根因：`schtasks` 的中文输出为 **GBK** 编码，Rust 侧用 `String::from_utf8_lossy` 按 UTF-8 解读，产生 mojibake（如 `ϵͳ�Ҳ���ָ�����ļ���`，实为「系统找不到指定的文件」）；乱码进一步导致「找不到」关键字匹配失效，无法命中「任务未注册」的友好分支。
  - 修复：新增 `run_schtasks()` 统一入口，前置 `chcp 65001` 强制 schtasks 以 UTF-8 输出，中文错误信息可正确解码与匹配。
- **错误提示前缀重复**
  - 原先 Rust 返回 `查询计划任务失败：…`，前端 `Settings.tsx` 又拼接 `查询失败：`，叠加成「查询失败：查询计划任务失败：…」。现 Rust 端只返回纯错误文案，前端前缀成为唯一前缀。
- **定时任务注册在普通用户下失败**
  - 移除 `schtasks /RL HIGHEST`（签到脚本只读写 `%APPDATA%` 并运行 Python，无需提权，强制最高权限会让普通用户卡在 Access Denied）；`/TR` 命令行改为 `cmd /c set "TRAEDATA_DIR=…" && "<python>" "<script>"`，对含空格的路径安全。
- **查询 / 取消操作静默吞错误**
  - `task_status` 原先无论成功失败都返回 `Ok(stdout)`，任务不存在时返回空串，界面显示空白；`task_unregister` 原先丢弃执行结果永远返回 `Ok(())`。现均真实上报结果：任务不存在时返回明确提示「未注册每日签到任务（请先在设置页点击「注册任务」）。」，取消时若任务本就不存在按已删除处理。

### 变更文件

| 文件 | 说明 |
|---|---|
| `src-python/device_proxy.py` | 上游代理链式转发（HTTP CONNECT / SOCKS5）、`tunnel_raw` 补 `200` 握手与 `502` 兜底 |
| `src-tauri/src/commands/proxy.rs` | 启动前捕获系统代理并注入 `UPSTREAM_PROXY`、停止时原样还原、抽出 `apply_proxy` / `get_existing_win_proxy` |
| `src-tauri/src/commands/misc.rs` | 新增 `run_schtasks()`（`chcp 65001`）、去重复前缀、移除 `/RL HIGHEST`、错误可见性增强 |
| `docs/issue-analysis-2026-08-16.md` | 新增问题深度分析报告（调用链、根因、修复、验证方法） |

### 升级注意

`device_proxy.py` 会被打包进安装包的 `resources/python/`，**代理相关修复必须重新执行 `npm run tauri build` 才会进入正式版**；开发模式 `npm run tauri dev` 直接读取 `src-python/`，重启代理即生效。

---

## [2.4.2] - 2026-08-15

### 修复
- 修复发布版黑框 / 闪退 / 排队提醒丢失等 GUI 失灵问题。
- 健康检查端点统一为 `/health`，文档英文化与路径清理。
- 移除 `proxy_logs` 目录引用，日志统一存放在 `logs/` 下。
- 全面修复文档错误；恢复误删的 `src-python/tests/test_auto_checkin.py`。

### 新增
- 便携版打包脚本 `scripts/make_portable_zip.py` / `scripts/package_portable.py`。

---

## [2.4.1] - 2026-08-14

### 变更
- 项目重命名为 `trae-work-assistant`，同步更新文档与用户手册。

### 新增
- 账号切换流程重构、保存登录态能力、帮助说明。

### 修复
- 日志相关问题修复。

---

## [2.4.0]

- API 服务页面重构、日志页面整合与 UI 优化。

## [2.3.0]

- API 服务协议对齐、代理修复、交互优化与日志增强。

## [2.2.0]

- 全面质量优化：Mutex 安全锁（poison 恢复）、竞态修复、暗色模式图表适配、积分三线趋势图。

## [2.0.0]

- 本地 API 网关（axum + ureq）、SSE 协议转换、账号池智能调度、签到错误冷却状态机、6 层设备标识重置。
