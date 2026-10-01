# Windows 完整安装包

版本 2.4.7 同时提供 MSI 和 NSIS EXE，两者包含同一应用与运行时。选择一个安装即可。

## 授权集成

本项目的 `src-tauri/src/license_guard/` 是 `D:\My_Codeproject\license-guard\client\license_guard` 授权协议的 Rust 实现。它在生产构建中启用激活入口、RSA-SHA256 公钥验签、机器绑定、过期和时钟回拨检查。公钥与该项目完全一致，编译到可执行文件中；私钥、授权口令、本机 license.dat 和用户账号数据均不进入安装包。

license-guard 原项目的 PyInstaller 示例适用于 Python 应用；本项目使用 Tauri 原生 WiX / NSIS 打包，保留已有授权实现，不使用示例 Python 程序替代桌面应用。首次激活仍需要授权服务器可达和有效口令。

## 内置组件

- 官方 Windows x64 Python 3.13.16，包括标准库、OpenSSL、SQLite、VC 运行库 DLL。
- `src-python/requirements-release.txt` 固定的 cryptography、pywin32、Brotli、cffi 与 pycparser。
- 完整 Chrome for Testing 154.0.8037.92，包括 DLL、区域语言和数据文件。
- WebView2 x64 离线安装程序，由 Tauri 安装器按需执行。
- Python 辅助脚本、PowerShell 切换桥、Rust 原生后端和预构建前端。

系统支持 Windows 10/11 x64。用户无需安装开发工具。Trae / WorkBuddy 属于被管理的外部客户端，仍需单独安装；首次授权、签到及其他在线功能需要网络。

## 复现构建

构建机安装 Node.js、Rust MSVC、VS C++ Build Tools、Python 3.11+ 和 pip，然后执行：

```powershell
npm ci
npm run release:prepare
python src-python/tests/test_auto_checkin.py
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri -- build
```

构建前会自动准备运行时并检查依赖。下载缓存保存在 `release/cache/`，生成资源保存在 `resources/`；它们均排除在 Git 之外。Python 下载必须匹配官方发布 SHA256。检查在隔离 Python 模式和仅含 Windows System32 的 PATH 下执行，包含 DPAPI、AES-GCM、Brotli、SSL、SQLite、代理 CA 生成，以及独立浏览器启动。

`release/runtime-manifest.json` 列出组件版本和每个资源文件的 SHA256。构建完成后执行 `python scripts/verify-installers.py`，使用构建机的 WiX `dark.exe` 与 7-Zip 提取 MSI / EXE，逐文件核对清单并运行内置环境检查；此过程不安装应用。Release 中的 `SHA256SUMS.txt` 提供最终安装包校验值。GitHub 发布工作流也会执行同一检查。

验证后执行 `python scripts/prepare-release-upload.py`，只选取验证报告中的两个安装器，生成 `release/upload/` 下的英文文件名、资源清单、验证报告和校验文件。这样可避免 GitHub 自动修改中文文件名造成校验清单与下载文件名不一致，也不会误上传构建缓存中的旧安装器。
