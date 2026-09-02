<#
.SYNOPSIS
    Trae Work 账号切换集成桥（非交互模式）
.DESCRIPTION
    供 Trae Work 助手（Tauri）调用的非交互切换层。
    封装「关闭 TRAE → 恢复目标账号登录态 → 重置机器码 → 启动 TRAE」流程，
    并以 NDJSON 逐行输出进度，供桌面端渲染步骤条。

    注意：本脚本是集成层，封装账号切换与设备标识重置的全部逻辑，
    供 Trae Work 助手（Tauri）以非交互模式调用。

.PARAMETER Action
    Switch（切换账号）/ ResetMachineId（仅重置机器码）/ BackupCurrent（备份当前）

.PARAMETER UserId
    目标账号的 UserID（16 位数字，与 checkin_accounts.json 的 UserID 对齐）

.PARAMETER Json
    以 NDJSON 输出进度（每行一个 JSON 对象）
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Switch', 'ResetMachineId', 'BackupCurrent', 'RestoreOnly', 'ResetDeviceIds', 'SaveCurrentLogin')]
    [string]$Action,

    [Parameter(Mandatory = $false)]
    [string]$UserId,

    [Parameter(Mandatory = $false)]
    [switch]$Json
)

# 注意：不要在此加 `#Requires -RunAsAdministrator`。
# 仅 Reset-MachineId 写 HKLM 需管理员；Switch/Backup 普通用户即可运行。
# 若强制要求管理员，普通权限启动的 App 调起脚本会直接 ScriptRequiresElevation 失败。

$ErrorActionPreference = 'Stop'

$Script:TraeDataDir = "$env:APPDATA\TRAE SOLO CN"
$Script:AppDataDir = "$env:APPDATA\TraeWorkAssistant"
$Script:ProfilesDir = "$Script:AppDataDir\data\profiles"
$Script:CurrentAccountFile = "$Script:ProfilesDir\current_account.txt"
$Script:LogFile = "$Script:AppDataDir\logs\switcher.log"
$Script:_TraeExeCache = $null

function Find-TraeExe {
    # ── 顺序原则（修复「首次切换误用 Trae CN.exe」）─────────────────────────────
    # 旧逻辑把「运行中进程」作为最高优先级，导致残留/错误的 Trae 进程（如旧的
    # Trae CN.exe）被优先采用，从而启动错误的 exe。现改为：
    #   1) 用户显式配置 > 2) 候选路径 > 3) 开始菜单/桌面 lnk > 4) 注册表
    #   > 5) 运行中进程（最后回退）> 6) 进程缓存（兜底，仅自定义安装且当前未运行时）
    # 这样正常情况下总是解析到用户真实安装的 TRAE SOLO CN，而非被残留进程带偏。

    # 1. 用户显式配置路径（最高优先级）
    $settingsFile = Join-Path $Script:AppDataDir 'conf\app_settings.json'
    if (Test-Path $settingsFile) {
        try {
            $settings = Get-Content $settingsFile -Raw | ConvertFrom-Json
            if ($settings.trae_path -and (Test-Path $settings.trae_path)) {
                $Script:_TraeExeCache = $settings.trae_path
                return $Script:_TraeExeCache
            }
        } catch {}
    }

    # 2. 多候选路径探测（与 Rust env.rs 保持一致）
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\TRAE SOLO CN\TRAE SOLO CN.exe",
        "$env:LOCALAPPDATA\Programs\TRAE SOLO\TRAE SOLO.exe",
        "$env:ProgramFiles\TRAE SOLO CN\TRAE SOLO CN.exe",
        "$env:ProgramFiles\TRAE SOLO\TRAE SOLO.exe",
        "$env:LOCALAPPDATA\Programs\Trae\Trae.exe",
        "$env:ProgramFiles\Trae\Trae.exe"
    )
    # 也检查 D 盘等非系统盘
    if ($env:ProgramFiles -notlike 'D:\*') {
        $candidates += 'D:\Programs\TRAE SOLO CN\TRAE SOLO CN.exe'
    }
    foreach ($c in $candidates) {
        if (Test-Path $c) {
            $Script:_TraeExeCache = $c
            return $Script:_TraeExeCache
        }
    }

    # 3. .lnk 快捷方式解析（开始菜单 / 桌面）
    try {
        $lnkDirs = @(
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs",
            "$env:ProgramData\Microsoft\Windows\Start Menu\Programs",
            "$env:USERPROFILE\Desktop",
            "$env:PUBLIC\Desktop"
        )
        $shell = New-Object -ComObject WScript.Shell
        foreach ($dir in $lnkDirs) {
            if (-not (Test-Path $dir)) { continue }
            $lnks = Get-ChildItem -Path $dir -Filter '*.lnk' -Recurse -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -like '*TRAE*' -or $_.Name -like '*Trae*' }
            foreach ($lnk in $lnks) {
                $shortcut = $shell.CreateShortcut($lnk.FullName)
                if ($shortcut.TargetPath -and (Test-Path $shortcut.TargetPath)) {
                    $Script:_TraeExeCache = $shortcut.TargetPath
                    return $Script:_TraeExeCache
                }
            }
        }
    } catch {}

    # 4. 注册表回退
    try {
        $regKeys = @(
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
            'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
        )
        foreach ($key in $regKeys) {
            $items = Get-ItemProperty $key -ErrorAction SilentlyContinue |
                Where-Object { $_.DisplayName -like '*TRAE*' -or $_.DisplayName -like '*Trae*' }
            foreach ($item in $items) {
                # 尝试 DisplayIcon
                if ($item.DisplayIcon) {
                    $iconPath = $item.DisplayIcon -replace ',', ''
                    $iconPath = $iconPath.Trim()
                    if (Test-Path $iconPath) {
                        $Script:_TraeExeCache = $iconPath
                        return $Script:_TraeExeCache
                    }
                }
                # 尝试 InstallLocation
                if ($item.InstallLocation) {
                    $loc = $item.InstallLocation.Trim()
                    $exeCandidates = @(
                        (Join-Path $loc 'TRAE SOLO CN.exe'),
                        (Join-Path $loc 'TRAE SOLO.exe'),
                        (Join-Path $loc 'Trae.exe')
                    )
                    foreach ($exe in $exeCandidates) {
                        if (Test-Path $exe) {
                            $Script:_TraeExeCache = $exe
                            return $Script:_TraeExeCache
                        }
                    }
                }
            }
        }
    } catch {}

    # 5. 运行中进程（最后回退之一）：仅当以上都找不到时才用，
    #    避免残留/错误的 Trae 进程误导启动路径。同时排除本助手自身进程
    #    （进程名以 "Trae" 开头，如 "Trae Work 助手"），避免把 App 本体当成 Trae 启动。
    try {
        $selfPid = $PID
        $parentPid = $selfPid
        $KnownAppName = 'Trae Work 助手'
        $parentName = $KnownAppName
        try {
            $pp = (Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $selfPid" -ErrorAction SilentlyContinue).ParentProcessId
            if ($pp) {
                $parentPid = $pp
                $pproc = Get-Process -Id $parentPid -ErrorAction SilentlyContinue
                if ($pproc) { $parentName = $pproc.Name }
            }
        } catch {}
        $proc = Get-Process -Name 'Trae*','TRAE*' -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^(Trae|TRAE)' -and $_.Path -and $_.Id -ne $selfPid -and $_.Id -ne $parentPid -and $_.Name -ne $parentName }
        if ($proc) {
            $exePath = $proc | Select-Object -First 1 -ExpandProperty Path
            if ($exePath -and (Test-Path $exePath)) {
                $Script:_TraeExeCache = $exePath
                return $Script:_TraeExeCache
            }
        }
    } catch {}

    # 6. 进程缓存兜底（自定义安装、当前未运行、且上述均未命中时）
    if ($Script:_TraeExeCache -and (Test-Path $Script:_TraeExeCache)) {
        return $Script:_TraeExeCache
    }

    return $null
}

function Write-Step {
    param([string]$Stage, [string]$Message, [string]$Status = 'info')
    $obj = [ordered]@{
        stage   = $Stage
        status  = $Status
        message = $Message
        time    = (Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
    } | ConvertTo-Json -Compress
    if ($Json) {
        $obj | Out-Host
    } else {
        Write-Host "[$Stage] $Message"
    }
    try {
        if (-not (Test-Path (Split-Path $Script:LogFile))) { New-Item -ItemType Directory -Path (Split-Path $Script:LogFile) -Force | Out-Null }
        Add-Content -Path $Script:LogFile -Value "[$((Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))] [$Stage] $Message" -Encoding UTF8
    } catch {}
}

function Get-CurrentAccount {
    if (Test-Path $Script:CurrentAccountFile) {
        try {
            $id = (Get-Content $Script:CurrentAccountFile -Raw).Trim()
            if ($id) { return $id }
        } catch {}
    }
    return $null
}

function Set-CurrentAccount {
    param([string]$AccountId)
    try {
        $dir = Split-Path $Script:CurrentAccountFile
        if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
        Set-Content -Path $Script:CurrentAccountFile -Value $AccountId -NoNewline -Encoding UTF8
    } catch {}
}

function Stop-Trae {
    # 排除本助手自身进程：本应用进程名以 "Trae" 开头（如 "Trae Work 助手"），
    # 若不过滤会被 Get-Process -Name 'Trae*' 命中并被 Stop-Process 误杀，导致 App 直接退出。
    $selfPid = $PID
    $parentPid = $selfPid
    $KnownAppName = 'Trae Work 助手'
    $parentName = $KnownAppName
    try {
        $pp = (Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $selfPid" -ErrorAction SilentlyContinue).ParentProcessId
        if ($pp) {
            $parentPid = $pp
            $pproc = Get-Process -Id $parentPid -ErrorAction SilentlyContinue
            if ($pproc) { $parentName = $pproc.Name }
        }
    } catch {}
    $p = Get-Process -Name 'Trae*' -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -match '^(Trae|TRAE)' -and $_.Id -ne $selfPid -and $_.Id -ne $parentPid -and $_.Name -ne $parentName
    }
    if ($p) {
        Write-Step -Stage 'stop' -Message '正在关闭 Trae Work' -Status 'running'
        # 在关闭前缓存 exe 路径，供 Start-Trae 使用
        $exePath = $p | Select-Object -First 1 -ExpandProperty Path -ErrorAction SilentlyContinue
        if ($exePath -and (Test-Path $exePath)) {
            $Script:_TraeExeCache = $exePath
        }
        $p | Stop-Process -Force
        # 等待进程完全退出，最多等 8 秒
        $waited = 0
        while ($waited -lt 8) {
            Start-Sleep -Seconds 1
            $waited++
            $still = Get-Process -Name 'Trae*' -ErrorAction SilentlyContinue | Where-Object {
                $_.Name -match '^(Trae|TRAE)' -and $_.Id -ne $selfPid -and $_.Id -ne $parentPid -and $_.Name -ne $parentName
            }
            if (-not $still) { break }
        }
        if ($waited -ge 8) {
            Write-Step -Stage 'stop' -Message "进程未在 $waited 秒内退出，可能仍有文件锁" -Status 'warn'
        }
    } else {
        Write-Step -Stage 'stop' -Message 'Trae Work 未运行' -Status 'skip'
        # 进程未运行时也尝试查找 exe 路径并缓存
        if (-not $Script:_TraeExeCache) {
            $found = Find-TraeExe
            if ($found) {
                $Script:_TraeExeCache = $found
            }
        }
    }
}

function Start-Trae {
    $exe = Find-TraeExe
    if (-not $exe) {
        Write-Step -Stage 'start' -Message '未找到 TRAE 安装路径，请在设置中指定' -Status 'error'
        throw '未找到 TRAE 可执行文件'
    }
    Write-Step -Stage 'start' -Message "正在启动 Trae Work: $exe" -Status 'running'
    Start-Process -FilePath $exe -WindowStyle Normal
}

function Reset-MachineId {
    # 重置 6 层机器码中的 MachineGuid（需管理员）。非管理员时跳过并提示，不阻断切换。
    $newGuid = (New-Guid).Guid
    try {
        Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Cryptography' -Name 'MachineGuid' -Value $newGuid -Force
        Write-Step -Stage 'machine' -Message "机器码已重置为 $newGuid" -Status 'ok'
    } catch {
        Write-Step -Stage 'machine' -Message "重置机器码需要管理员权限，已跳过（不影响账号切换）: $_" -Status 'skip'
    }
}

function Reset-DeviceIdsOnly {
    <#
    .SYNOPSIS
        6 层设备标识重置（本项目自主设计）
    .DESCRIPTION
        1. machineid 文件 → 新 hex32 UUID
        2. storage.json telemetry.machineId / telemetry.sqmId → 替换
        3. storage.json aha.device.device_id → 替换
        4. aha/TinyStorage device_id → 清除
        5. 注册表 MachineGuid → 替换（需管理员）
        6. trae-webview 追踪数据 → 清除
        额外：删除 has_device_id_updated_to_aha 标记位
    #>
    $traeDir = $Script:TraeDataDir
    if (-not (Test-Path $traeDir)) {
        Write-Step -Stage 'device' -Message "TRAE 数据目录不存在: $traeDir" -Status 'error'
        return
    }

    $newMachineId = -join ((1..32) | ForEach-Object { '{0:x}' -f (Get-Random -Maximum 16) })
    $newDeviceId = -join ((1..15) | ForEach-Object { Get-Random -Maximum 10 })
    $newSqmId = (New-Guid).Guid
    $resetCount = 0

    # 1. machineid 文件
    $machineIdFile = Join-Path $traeDir 'machineid'
    if (Test-Path $machineIdFile) {
        try {
            Set-Content -Path $machineIdFile -Value $newMachineId -NoNewline -Encoding UTF8
            Write-Step -Stage 'device' -Message "[1/6] machineid 已重置" -Status 'ok'
            $resetCount++
        } catch {
            Write-Step -Stage 'device' -Message "[1/6] machineid 重置失败: $_" -Status 'skip'
        }
    } else {
        Write-Step -Stage 'device' -Message "[1/6] machineid 文件不存在，跳过" -Status 'skip'
    }

    # 2 & 3. storage.json — telemetry.machineId / sqmId + aha.device.device_id
    # 注意：storage.json 在 User\globalStorage\ 下，且使用点号键名（非嵌套对象）
    $storageFile = Join-Path $traeDir 'User\globalStorage\storage.json'
    if (Test-Path $storageFile) {
        try {
            $raw = Get-Content $storageFile -Raw
            $storage = $raw | ConvertFrom-Json
            $changed = $false
            # 点号键名访问：$storage.'telemetry.machineId' 而非 $storage.telemetry.machineId
            if ($storage.'telemetry.machineId' -ne $null) {
                $storage.'telemetry.machineId' = $newMachineId
                $changed = $true
            }
            if ($storage.'telemetry.sqmId' -ne $null) {
                $storage.'telemetry.sqmId' = $newSqmId
                $changed = $true
            }
            if ($storage.'aha.device.device_id' -ne $null) {
                $storage.'aha.device.device_id' = $newDeviceId
                $changed = $true
            }
            # 删除 has_device_id_updated_to_aha 标记位
            if ($storage.'has_device_id_updated_to_aha' -ne $null) {
                $storage.PSObject.Properties.Remove('has_device_id_updated_to_aha')
                $changed = $true
            }
            if ($changed) {
                $storage | ConvertTo-Json -Depth 20 | Set-Content -Path $storageFile -Encoding UTF8
                Write-Step -Stage 'device' -Message "[2/3] storage.json 设备标识已重置" -Status 'ok'
                $resetCount++
            } else {
                Write-Step -Stage 'device' -Message "[2/3] storage.json 无需修改" -Status 'skip'
            }
        } catch {
            Write-Step -Stage 'device' -Message "[2/3] storage.json 重置失败: $_" -Status 'skip'
        }
    } else {
        Write-Step -Stage 'device' -Message "[2/3] storage.json 不存在，跳过" -Status 'skip'
    }

    # 4. aha/TinyStorage device_id — 清除
    $tinyStorageDir = Join-Path $traeDir 'aha\TinyStorage'
    if (Test-Path $tinyStorageDir) {
        try {
            $tinyFiles = Get-ChildItem -Path $tinyStorageDir -Recurse -File -ErrorAction SilentlyContinue
            foreach ($f in $tinyFiles) {
                $content = Get-Content $f.FullName -Raw -ErrorAction SilentlyContinue
                if ($content -and $content -match 'device_id') {
                    Remove-Item $f.FullName -Force
                }
            }
            Write-Step -Stage 'device' -Message "[4/6] aha/TinyStorage device_id 已清除" -Status 'ok'
            $resetCount++
        } catch {
            Write-Step -Stage 'device' -Message "[4/6] aha/TinyStorage 清除失败: $_" -Status 'skip'
        }
    } else {
        Write-Step -Stage 'device' -Message "[4/6] aha/TinyStorage 目录不存在，跳过" -Status 'skip'
    }

    # 5. 注册表 MachineGuid（需管理员）
    try {
        Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Cryptography' -Name 'MachineGuid' -Value $newSqmId -Force
        Write-Step -Stage 'device' -Message "[5/6] 注册表 MachineGuid 已重置" -Status 'ok'
        $resetCount++
    } catch {
        Write-Step -Stage 'device' -Message "[5/6] 注册表 MachineGuid 重置需要管理员权限，已跳过" -Status 'skip'
    }

    # 6. trae-webview 追踪数据（Cookies/Local Storage/Session Storage）
    $webviewDir = Join-Path $traeDir 'Partitions\trae-webview'
    if (Test-Path $webviewDir) {
        try {
            $clearDirs = @('Network', 'Local Storage', 'Session Storage')
            foreach ($d in $clearDirs) {
                $target = Join-Path $webviewDir $d
                if (Test-Path $target) {
                    Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue
                }
            }
            Write-Step -Stage 'device' -Message "[6/6] trae-webview 追踪数据已清除" -Status 'ok'
            $resetCount++
        } catch {
            Write-Step -Stage 'device' -Message "[6/6] trae-webview 清除失败: $_" -Status 'skip'
        }
    } else {
        Write-Step -Stage 'device' -Message "[6/6] trae-webview 目录不存在，跳过" -Status 'skip'
    }

    Write-Step -Stage 'device' -Message "6 层设备标识重置完成（$resetCount/6 层成功）" -Status $(if ($resetCount -ge 4) { 'ok' } else { 'info' })
}

function Backup-CurrentProfile {
    param([string]$Slot)
    $dest = Join-Path $Script:ProfilesDir $Slot
    if (-not (Test-Path $Script:TraeDataDir)) {
        Write-Step -Stage 'backup' -Message '当前数据目录不存在，跳过备份' -Status 'skip'
        return
    }
    if (-not (Test-Path $dest)) { New-Item -ItemType Directory -Path $dest -Force | Out-Null }
    $src = $Script:TraeDataDir
    $copied = 0

    # 精准备份：仅复制登录态关键文件（参考 traework-switcher）
    # 1. storage.json — 设备标识、遥测、认证信息
    $storageSrc = "$src\User\globalStorage\storage.json"
    if (Test-Path $storageSrc) { $dir = Split-Path "$dest\User\globalStorage\storage.json" -Parent; New-Item -ItemType Directory -Force -Path $dir | Out-Null; Copy-Item $storageSrc "$dest\User\globalStorage\storage.json" -Force; $copied++ }

    # 2. state.vscdb — 登录令牌数据库
    $stateDbSrc = "$src\User\globalStorage\state.vscdb"
    if (Test-Path $stateDbSrc) { $dir = Split-Path "$dest\User\globalStorage\state.vscdb" -Parent; New-Item -ItemType Directory -Force -Path $dir | Out-Null; Copy-Item $stateDbSrc "$dest\User\globalStorage\state.vscdb" -Force; $copied++ }
    $stateDbBak = "$src\User\globalStorage\state.vscdb.backup"
    if (Test-Path $stateDbBak) { Copy-Item $stateDbBak "$dest\User\globalStorage\state.vscdb.backup" -Force; $copied++ }

    # 3. machineid — 机器标识
    $machineIdSrc = "$src\machineid"
    if (Test-Path $machineIdSrc) { Copy-Item $machineIdSrc "$dest\machineid" -Force; $copied++ }

    # 4. aha\ — 设备认证数据
    $ahaSrc = "$src\aha"
    if (Test-Path $ahaSrc) { $ahaDest = "$dest\aha"; if (Test-Path $ahaDest) { Remove-Item $ahaDest -Recurse -Force -ErrorAction SilentlyContinue }; Copy-Item $ahaSrc $ahaDest -Recurse -Force -ErrorAction SilentlyContinue; $copied++ }

    # 5. Preferences / Local State
    if (Test-Path "$src\Preferences") { Copy-Item "$src\Preferences" "$dest\Preferences" -Force; $copied++ }
    if (Test-Path "$src\Local State") { Copy-Item "$src\Local State" "$dest\Local State" -Force; $copied++ }

    # 6. Local Storage\leveldb + config.db
    $lsSrc = "$src\Local Storage\leveldb"
    if (Test-Path $lsSrc) { $lsDest = "$dest\Local Storage\leveldb"; New-Item -ItemType Directory -Force -Path $lsDest | Out-Null; Copy-Item "$lsSrc\*" $lsDest -Force -ErrorAction SilentlyContinue; $copied++ }
    $lsConfig = "$src\Local Storage\config.db"
    if (Test-Path $lsConfig) { $lsParent = "$dest\Local Storage"; if (-not (Test-Path $lsParent)) { New-Item -ItemType Directory -Force -Path $lsParent | Out-Null }; Copy-Item $lsConfig "$lsParent\config.db" -Force; $copied++ }

    # 7. Network\
    $netSrc = "$src\Network"
    if (Test-Path $netSrc) { $netDest = "$dest\Network"; if (Test-Path $netDest) { Remove-Item $netDest -Recurse -Force -ErrorAction SilentlyContinue }; Copy-Item $netSrc $netDest -Recurse -Force -ErrorAction SilentlyContinue; $copied++ }

    # 8. Partitions\trae-webview + icube-web-crawler
    $wvSrc = "$src\Partitions\trae-webview"
    if (Test-Path $wvSrc) { $wvDest = "$dest\Partitions\trae-webview"; if (Test-Path $wvDest) { Remove-Item $wvDest -Recurse -Force -ErrorAction SilentlyContinue }; $wvParent = Split-Path $wvDest -Parent; New-Item -ItemType Directory -Force -Path $wvParent | Out-Null; Copy-Item $wvSrc $wvDest -Recurse -Force -ErrorAction SilentlyContinue; $copied++ }
    $icSrc = "$src\Partitions\icube-web-crawler-shared-session-v1.0"
    if (Test-Path $icSrc) { $icDest = "$dest\Partitions\icube-web-crawler-shared-session-v1.0"; if (Test-Path $icDest) { Remove-Item $icDest -Recurse -Force -ErrorAction SilentlyContinue }; $icParent = Split-Path $icDest -Parent; New-Item -ItemType Directory -Force -Path $icParent | Out-Null; Copy-Item $icSrc $icDest -Recurse -Force -ErrorAction SilentlyContinue; $copied++ }

    # 9. Session Storage\
    $ssSrc = "$src\Session Storage"
    if (Test-Path $ssSrc) { $ssDest = "$dest\Session Storage"; if (Test-Path $ssDest) { Remove-Item $ssDest -Recurse -Force -ErrorAction SilentlyContinue }; Copy-Item $ssSrc $ssDest -Recurse -Force -ErrorAction SilentlyContinue; $copied++ }

    Write-Step -Stage 'backup' -Message "已备份当前登录态到 $Slot ($copied 项)" -Status 'ok'
}

function Restore-Profile {
    param([string]$Slot)
    $src = Join-Path $Script:ProfilesDir $Slot
    if (-not (Test-Path $src)) {
        Write-Step -Stage 'restore' -Message "目标账号 $Slot 无快照，请先登录该账号并保存登录态" -Status 'error'
        throw "目标账号 $Slot 无快照"
    }
    $dest = $Script:TraeDataDir
    if (-not (Test-Path $dest)) { New-Item -ItemType Directory -Path $dest -Force | Out-Null }
    $restored = 0

    # 删除 code.lock 防止启动冲突
    $codeLock = "$dest\code.lock"
    if (Test-Path $codeLock) { Remove-Item $codeLock -Force -ErrorAction SilentlyContinue }

    # 精准恢复：仅恢复登录态关键文件（与 Backup-CurrentProfile 对称）
    # 1. storage.json
    if (Test-Path "$src\User\globalStorage\storage.json") { $dir = "$dest\User\globalStorage"; if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }; Copy-Item "$src\User\globalStorage\storage.json" "$dir\storage.json" -Force; $restored++ }

    # 2. state.vscdb + backup
    if (Test-Path "$src\User\globalStorage\state.vscdb") { $dir = "$dest\User\globalStorage"; if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }; Copy-Item "$src\User\globalStorage\state.vscdb" "$dir\state.vscdb" -Force; $restored++ }
    if (Test-Path "$src\User\globalStorage\state.vscdb.backup") { Copy-Item "$src\User\globalStorage\state.vscdb.backup" "$dest\User\globalStorage\state.vscdb.backup" -Force; $restored++ }

    # 3. machineid
    if (Test-Path "$src\machineid") { Copy-Item "$src\machineid" "$dest\machineid" -Force; $restored++ }

    # 4. aha\
    if (Test-Path "$src\aha") { $target = "$dest\aha"; if (Test-Path $target) { Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue }; Copy-Item "$src\aha" $target -Recurse -Force -ErrorAction SilentlyContinue; $restored++ }

    # 5. Preferences / Local State
    if (Test-Path "$src\Preferences") { Copy-Item "$src\Preferences" "$dest\Preferences" -Force; $restored++ }
    if (Test-Path "$src\Local State") { Copy-Item "$src\Local State" "$dest\Local State" -Force; $restored++ }

    # 6. Local Storage\leveldb + config.db
    if (Test-Path "$src\Local Storage\leveldb") { $target = "$dest\Local Storage\leveldb"; if (-not (Test-Path $target)) { New-Item -ItemType Directory -Force -Path $target | Out-Null } else { Remove-Item "$target\*" -Force -ErrorAction SilentlyContinue }; Copy-Item "$src\Local Storage\leveldb\*" $target -Force -ErrorAction SilentlyContinue; $restored++ }
    if (Test-Path "$src\Local Storage\config.db") { $target = "$dest\Local Storage"; if (-not (Test-Path $target)) { New-Item -ItemType Directory -Force -Path $target | Out-Null }; Copy-Item "$src\Local Storage\config.db" "$target\config.db" -Force; $restored++ }

    # 7. Network\
    if (Test-Path "$src\Network") { $target = "$dest\Network"; if (Test-Path $target) { Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue }; Copy-Item "$src\Network" $target -Recurse -Force -ErrorAction SilentlyContinue; $restored++ }

    # 8. Partitions\trae-webview + icube-web-crawler
    if (Test-Path "$src\Partitions\trae-webview") { $target = "$dest\Partitions\trae-webview"; if (Test-Path $target) { Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue }; $pParent = Split-Path $target -Parent; if (-not (Test-Path $pParent)) { New-Item -ItemType Directory -Force -Path $pParent | Out-Null }; Copy-Item "$src\Partitions\trae-webview" $target -Recurse -Force -ErrorAction SilentlyContinue; $restored++ }
    if (Test-Path "$src\Partitions\icube-web-crawler-shared-session-v1.0") { $target = "$dest\Partitions\icube-web-crawler-shared-session-v1.0"; if (Test-Path $target) { Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue }; $pParent = Split-Path $target -Parent; if (-not (Test-Path $pParent)) { New-Item -ItemType Directory -Force -Path $pParent | Out-Null }; Copy-Item "$src\Partitions\icube-web-crawler-shared-session-v1.0" $target -Recurse -Force -ErrorAction SilentlyContinue; $restored++ }

    # 9. Session Storage\
    if (Test-Path "$src\Session Storage") { $target = "$dest\Session Storage"; if (Test-Path $target) { Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue }; Copy-Item "$src\Session Storage" $target -Recurse -Force -ErrorAction SilentlyContinue; $restored++ }

    Write-Step -Stage 'restore' -Message "已恢复账号 $Slot 的登录态 ($restored 项)" -Status 'ok'
}

# ============ 入口 ============
try {
    if (-not $UserId -and $Action -ne 'ResetMachineId' -and $Action -ne 'ResetDeviceIds') {
        Write-Step -Stage 'init' -Message '缺少 -UserId 参数' -Status 'error'
        exit 1
    }
    Write-Step -Stage 'init' -Message "开始操作: $Action (userId=$UserId)" -Status 'info'

    switch ($Action) {
        'Switch' {
            # 预检查：目标账号是否有快照（在关闭 Trae 之前检查）
            $targetProfile = Join-Path $Script:ProfilesDir $UserId
            if (-not (Test-Path $targetProfile)) {
                Write-Step -Stage 'fatal' -Message "目标账号 $UserId 无快照，请先登录该账号并点击「保存当前登录态」" -Status 'error'
                exit 1
            }
            Stop-Trae
            # 保存当前登录态到 "last" 槽位（安全备份）
            Backup-CurrentProfile -Slot 'last'
            # 如果知道当前账号 ID，也备份到该账号的槽位（用于下次切回）
            $currentAcct = Get-CurrentAccount
            if ($currentAcct -and $currentAcct -ne $UserId) {
                Backup-CurrentProfile -Slot $currentAcct
                Write-Step -Stage 'backup' -Message "当前账号 $currentAcct 的登录态已备份" -Status 'ok'
            }
            # 恢复目标账号的登录态（含设备标识）
            Restore-Profile -Slot $UserId
            # 记录当前账号 ID
            Set-CurrentAccount -AccountId $UserId
            Start-Trae
            Write-Step -Stage 'done' -Message "已切换至账号 $UserId" -Status 'ok'
        }
        'SaveCurrentLogin' {
            # 保存当前登录态：关闭 Trae → 备份 → 启动
            Stop-Trae
            Backup-CurrentProfile -Slot $UserId
            Set-CurrentAccount -AccountId $UserId
            Start-Trae
            Write-Step -Stage 'done' -Message "已保存账号 $UserId 的当前登录态" -Status 'ok'
        }
        'ResetMachineId' {
            Reset-MachineId
            Write-Step -Stage 'done' -Message '机器码已重置' -Status 'ok'
        }
        'ResetDeviceIds' {
            Reset-DeviceIdsOnly
            Write-Step -Stage 'done' -Message '6 层设备标识重置完成' -Status 'ok'
        }
        'BackupCurrent' {
            Backup-CurrentProfile -Slot $UserId
            Set-CurrentAccount -AccountId $UserId
            Write-Step -Stage 'done' -Message '备份完成' -Status 'ok'
        }
        'RestoreOnly' {
            Stop-Trae
            Restore-Profile -Slot $UserId
            Set-CurrentAccount -AccountId $UserId
            Start-Trae
            Write-Step -Stage 'done' -Message "已恢复账号 $UserId 的登录态" -Status 'ok'
        }
    }
    exit 0
} catch {
    Write-Step -Stage 'fatal' -Message "失败: $_" -Status 'error'
    exit 1
}
