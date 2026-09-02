@echo off
title Trae Work Assistant - Launcher

cd /d "%~dp0"

echo ============================================
echo   Trae Work Assistant - One Click Start
echo ============================================
echo.

:: Check Node.js
where node >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Node.js not found. Install it from: https://nodejs.org/
    pause
    exit /b 1
)

:: Check Rust / Cargo (required to build Tauri backend)
where cargo >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Rust not found. Install it from: https://rustup.rs/
    pause
    exit /b 1
)

:: Install npm dependencies on first run
if not exist "node_modules" (
    echo [STEP] node_modules not found, installing npm dependencies...
    call npm install
    if errorlevel 1 (
        echo [ERROR] npm install failed
        pause
        exit /b 1
    )
    echo.
)

echo [START] Launching Tauri dev mode (first Rust build may take a while)...
echo.
call npm run tauri dev

if errorlevel 1 (
    echo.
    echo [EXIT] Application exited with an error
) else (
    echo.
    echo [DONE] Application closed
)
pause
