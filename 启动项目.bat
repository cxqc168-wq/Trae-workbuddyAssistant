@echo off
chcp 936 >nul
title Trae Work 助手 - 一键启动

rem 切换到脚本所在目录（项目根目录）
cd /d "%~dp0"

echo ==========================================
echo   Trae Work 助手 - 一键启动
echo ==========================================
echo.

rem ---- 环境检查 ----
where node >nul 2>nul
if errorlevel 1 (
    echo [错误] 未检测到 Node.js，请先安装：https://nodejs.org/
    goto :fail
)

where cargo >nul 2>nul
if errorlevel 1 (
    echo [错误] 未检测到 Rust，请先安装：https://rustup.rs/
    echo        （需要 MSVC 工具链 + VS Build Tools C++ 组件）
    goto :fail
)

echo [1/3] Node.js 与 Rust 环境检查通过
echo.

rem ---- 依赖检查：首次运行自动安装 ----
if not exist "node_modules" (
    echo [2/3] 首次运行，正在安装 npm 依赖（约 1-3 分钟）...
    call npm install
    if errorlevel 1 (
        echo [错误] npm 依赖安装失败，请检查网络后重试
        goto :fail
    )
) else (
    echo [2/3] npm 依赖已就绪
)
echo.

rem ---- 启动应用 ----
echo [3/3] 正在启动应用（Rust 增量编译约 20-60 秒，首次编译更久）...
echo       启动后会自动弹出桌面窗口，本窗口请保持开启（关闭即退出应用）
echo.

call npm run tauri dev
if errorlevel 1 (
    echo.
    echo [错误] 应用启动失败，请将上方错误信息反馈给开发者
    goto :fail
)

echo.
echo 应用已退出。
pause
exit /b 0

:fail
echo.
pause
exit /b 1
