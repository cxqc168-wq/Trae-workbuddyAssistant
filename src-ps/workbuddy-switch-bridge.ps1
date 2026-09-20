<#
.SYNOPSIS
    WorkBuddy 账号切换桥（非交互模式）—— v2：基于真实登录态文件
.DESCRIPTION
    供 Trae Work 助手（Tauri）调用。

    ── v2 说明（2026-09-20 重写）──────────────────────────────────────────────
    v1 假设「登录态 = Chromium 凭证层（Cookies / Local Storage / Local State…）」。
    实测证明这是错的：WorkBuddy 的登录态**与 webview 存储无关**，
    app\session\Network\Cookies 里只有 tgw_l7_route（负载均衡路由 cookie）和
    主题偏好，没有任何身份凭证。

    真实机制（从 WorkBuddy 自身 app.asar 源码 + 实盘文件双向确认）：
      登录态 = 单个明文 JSON 文件
        %LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info
      结构：{ account:{uid,nickname,uin,…}, auth:{accessToken,refreshToken,…},
              accounts:[…], allAccounts:[…] }
      源码位置：main/file-authentication-storage.js → getAuthSavePath()
        path.join(filePathService.sharedDataPath, "auth", `${authentication.id}.info`)
        sharedDataPath = %LOCALAPPDATA%\<EXTENSION_DATA_DIR_NAME>\Data\Public
        authentication.id = "workbuddy-desktop"

      另有一个「已登出标记」：<authFilePath>.logged-out
        只要该文件存在，应用会**故意忽略**登录态文件（源码 hasLogoutMarker()）。
        → 切换时必须把它删掉。

    ── 切换语义 ───────────────────────────────────────────────────────────────
      切换 = 把目标账号的登录态文件写回 workbuddy-desktop.info（并清掉登出标记）
      除此之外**不碰任何东西**：任务、会话、项目、历史记录、设置全部原样。
      这是天然的「只换凭证，数据不动」——因为应用本来就只从这一个文件读身份。

      应用还对该目录装了 fs.watch 监听（源码 initializeWatcher），
      外部改写后可能无需重启即生效；但本脚本仍走「关客户端→写→重启」的确定路径。

.PARAMETER Action
    Switch / SaveCurrentLogin / BackupCurrent / RestoreOnly /
    ShowPaths / ListProfiles / Fingerprint / SeedAuthSnapshots

.PARAMETER UserId
    Trae Work 助手 的账号行 ID（形如 wb-1a0a81926e5-3c686d53）。
    脚本会用 workbuddy_accounts.json 把它映射到真实 uid。

.PARAMETER Json
    以 NDJSON 逐行输出进度（每行一个 JSON 对象），供桌面端渲染步骤条

.PARAMETER DryRun
    只打印将要读写的文件，不做任何改动

.NOTES
    ⚠ 切换会关闭并重启 WorkBuddy 客户端。会在 WorkBuddy 里进行的会话就此中断。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Switch', 'SaveCurrentLogin', 'BackupCurrent', 'RestoreOnly',
                 'ShowPaths', 'ListProfiles', 'Fingerprint', 'SeedAuthSnapshots')]
    [string]$Action,

    [Parameter(Mandatory = $false)]
    [string]$UserId,

    [Parameter(Mandatory = $false)]
    [switch]$Json,

    [Parameter(Mandatory = $false)]
    [switch]$DryRun,

    [Parameter(Mandatory = $false)]
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

# ── 常量 ─────────────────────────────────────────────────────────────────────
$Script:WbDataDir   = Join-Path $env:USERPROFILE '.workbuddy'
$Script:AppDataDir  = Join-Path $env:APPDATA 'TraeWorkAssistant'
$Script:ProfilesDir = Join-Path $Script:AppDataDir 'data\profiles-wb'
$Script:AccountsDb  = Join-Path $Script:AppDataDir 'data\workbuddy_accounts.json'
$Script:CurrentAccountFile = Join-Path $Script:ProfilesDir 'current_account.txt'
$Script:LogFile     = Join-Path $Script:AppDataDir 'logs\workbuddy-switch.log'
$Script:_ExeCache   = $null
$Script:_MapCache   = $null

# 登录态文件所在的数据根（候选按顺序探测）
$Script:AuthRootCandidates = @(
    (Join-Path $env:LOCALAPPDATA 'CodeBuddyExtension\Data\Public'),
    (Join-Path $env:LOCALAPPDATA 'WorkBuddy\Data\Public')
)

function Get-AuthFile {
    foreach ($r in $Script:AuthRootCandidates) {
        $f = Join-Path (Join-Path $r 'auth') 'workbuddy-desktop.info'
        if (Test-Path -LiteralPath $f) { return $f }
    }
    # 都没找到时返回首选路径（供报错信息用）
    return Join-Path (Join-Path $Script:AuthRootCandidates[0] 'auth') 'workbuddy-desktop.info'
}

function Write-Step {
    param([string]$Stage, [string]$Message, [string]$Status = 'info')
    $obj = [ordered]@{
        stage   = $Stage
        status  = $Status
        message = $Message
        time    = (Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
    } | ConvertTo-Json -Compress
    if ($Json) { $obj | Out-Host } else { Write-Host "[$Stage] $Message" }
    try {
        $logDir = Split-Path $Script:LogFile
        if (-not (Test-Path -LiteralPath $logDir)) { New-Item -ItemType Directory -Path $logDir -Force | Out-Null }
        Add-Content -LiteralPath $Script:LogFile -Value "[$((Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))] [$Stage] $Message" -Encoding UTF8
    } catch {}
}

# ── 账号映射：slot id (wb-xxx)  <->  真实 uid ────────────────────────────────
function Get-AccountMap {
    if ($Script:_MapCache) { return $Script:_MapCache }
    $map = @{}
    if (Test-Path -LiteralPath $Script:AccountsDb) {
        try {
            $raw = Get-Content -LiteralPath $Script:AccountsDb -Raw -Encoding UTF8
            # 注意：PS 5.1 的 ConvertFrom-Json 遇到顶层 JSON 数组时不会展开，
            # 会把整个数组当成一个对象返回。必须显式判断后再展开，否则 $a.id
            # 会变成「所有 id 拼在一起的数组」。这是踩过的坑，别改回去。
            $parsed = $raw | ConvertFrom-Json
            $items = @()
            if ($parsed -is [System.Array]) { $items = $parsed }
            elseif ($null -ne $parsed) { $items = @($parsed) }
            foreach ($a in $items) {
                if ($a.id -and $a.uid) {
                    $map[[string]$a.id] = [pscustomobject]@{
                        Uid      = [string]$a.uid
                        Nickname = [string]$a.nickname
                    }
                }
            }
        } catch {
            Write-Step -Stage 'map' -Message "读取 workbuddy_accounts.json 失败（将跳过映射校验）: $_" -Status 'warn'
        }
    }
    $Script:_MapCache = $map
    return $map
}

function Get-SlotForUid {
    param([string]$Uid)
    $map = Get-AccountMap
    foreach ($k in $map.Keys) { if ($map[$k].Uid -eq $Uid) { return $k } }
    return $null
}

# ── 登录态文件读写 ───────────────────────────────────────────────────────────
function Read-AuthInfo {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    try {
        $txt = [System.IO.File]::ReadAllText($Path, [System.Text.Encoding]::UTF8)
        $j = $txt | ConvertFrom-Json
        return [pscustomobject]@{
            Raw      = $txt
            Uid      = [string]$j.account.uid
            Nickname = [string]$j.account.nickname
            Uin      = [string]$j.account.uin
            Domain   = [string]$j.auth.domain
            TokenLen = ([string]$j.auth.accessToken).Length
            RefreshLen = ([string]$j.auth.refreshToken).Length
            RefreshExpiresAt = $j.auth.refreshExpiresAt
            Json     = $j
        }
    } catch {
        return $null
    }
}

function Get-AuthMarkerPath {
    param([string]$AuthFilePath)
    return "$AuthFilePath.logged-out"
}

function Clear-LogoutMarker {
    param([string]$AuthFilePath)
    $m = Get-AuthMarkerPath -AuthFilePath $AuthFilePath
    if (Test-Path -LiteralPath $m) {
        try {
            Remove-Item -LiteralPath $m -Force -ErrorAction Stop
            Write-Step -Stage 'restore' -Message '已清除 .logged-out 登出标记（否则客户端会忽略登录态）' -Status 'info'
        } catch {
            Write-Step -Stage 'restore' -Message "清除登出标记失败: $_" -Status 'warn'
            return $false
        }
    }
    return $true
}

function Write-AuthFileAtomic {
    param([string]$Path, [string]$Content)
    $dir = Split-Path $Path -Parent
    if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    $tmp = Join-Path $dir ("workbuddy-desktop.tmp.{0}.{1}" -f $PID, ([guid]::NewGuid().ToString('N')))
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($tmp, $Content, $utf8NoBom)
    if (Test-Path -LiteralPath $Path) {
        # ★ PS 5.1 会把 $null 隐式转换成空字符串 "" 传给 .NET 的 string 形参，
        #   于是 File.Replace($tmp,$Path,"") 抛 ArgumentException「路径的形式不合法」。
        #   必须传一个真实的备份文件路径。
        $bak = $Path + '.prev'
        try {
            [System.IO.File]::Replace($tmp, $Path, $bak)
        }
        catch {
            Write-Step -Stage 'write' -Message ("Replace 失败，降级为移动写入: " + $_.Exception.Message) -Status 'warn'
            $bak2 = "{0}.prev-{1}" -f $Path, (Get-Date -Format 'yyyyMMdd-HHmmssfff')
            [System.IO.File]::Move($Path, $bak2)
            [System.IO.File]::Move($tmp, $Path)
        }
    }
    else { [System.IO.File]::Move($tmp, $Path) }
}

# ── 可执行文件定位 ───────────────────────────────────────────────────────────
function Find-WorkBuddyExe {
    if ($Script:_ExeCache -and (Test-Path -LiteralPath $Script:_ExeCache)) { return $Script:_ExeCache }
    try {
        $p = Get-Process -Name 'WorkBuddy' -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($p -and $p.Path -and (Test-Path -LiteralPath $p.Path)) { $Script:_ExeCache = $p.Path; return $Script:_ExeCache }
    } catch {}
    $candidates = @(
        'D:\Program Files\WorkBuddy\WorkBuddy.exe',
        'C:\Program Files\WorkBuddy\WorkBuddy.exe',
        "$env:LOCALAPPDATA\Programs\WorkBuddy\WorkBuddy.exe",
        "$env:ProgramFiles\WorkBuddy\WorkBuddy.exe"
    )
    foreach ($c in $candidates) { if (Test-Path -LiteralPath $c) { $Script:_ExeCache = $c; return $c } }
    try {
        $regKeys = @(
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
            'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
        )
        foreach ($key in $regKeys) {
            $items = Get-ItemProperty $key -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like '*WorkBuddy*' -or $_.DisplayName -like '*CodeBuddy*' }
            foreach ($item in $items) {
                if ($item.InstallLocation) {
                    $exe = Join-Path ($item.InstallLocation.Trim()) 'WorkBuddy.exe'
                    if (Test-Path -LiteralPath $exe) { $Script:_ExeCache = $exe; return $exe }
                }
                if ($item.DisplayIcon) {
                    $icon = ($item.DisplayIcon -replace ',.*$', '').Trim()
                    if ($icon -like '*.exe' -and (Test-Path -LiteralPath $icon)) { $Script:_ExeCache = $icon; return $icon }
                }
            }
        }
    } catch {}
    return $null
}

# ── 进程控制 ─────────────────────────────────────────────────────────────────
function Stop-WorkBuddy {
    $procs = @(Get-Process -Name 'WorkBuddy' -ErrorAction SilentlyContinue)
    if ($procs.Count -eq 0) {
        Write-Step -Stage 'stop' -Message 'WorkBuddy 客户端未运行' -Status 'skip'
    } else {
        Write-Step -Stage 'stop' -Message "正在关闭 WorkBuddy 客户端（$($procs.Count) 个进程）" -Status 'running'
        $main = $procs | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if ($main) {
            try { $null = $main.CloseMainWindow() } catch {}
            for ($i = 0; $i -lt 12; $i++) {
                Start-Sleep -Milliseconds 500
                if (-not (Get-Process -Id $main.Id -ErrorAction SilentlyContinue)) { break }
            }
        }
        $still = @(Get-Process -Name 'WorkBuddy' -ErrorAction SilentlyContinue)
        if ($still.Count -gt 0) { $still | Stop-Process -Force -ErrorAction SilentlyContinue }
        $waited = 0
        while ($waited -lt 40) {
            Start-Sleep -Milliseconds 500
            $waited++
            if (@(Get-Process -Name 'WorkBuddy' -ErrorAction SilentlyContinue).Count -eq 0) { break }
        }
        $left = @(Get-Process -Name 'WorkBuddy' -ErrorAction SilentlyContinue).Count
        if ($left -gt 0) {
            Write-Step -Stage 'stop' -Message "仍有 $left 个进程未退出；登录态文件可能仍被占用" -Status 'warn'
        } else {
            Write-Step -Stage 'stop' -Message 'WorkBuddy 客户端已完全退出' -Status 'ok'
        }
    }
    $lock = Join-Path $Script:WbDataDir 'app\lockfile'
    if (Test-Path -LiteralPath $lock) {
        try { Remove-Item -LiteralPath $lock -Force -ErrorAction Stop; Write-Step -Stage 'stop' -Message '已清理残留 lockfile' -Status 'info' } catch {}
    }
}

function Start-WorkBuddy {
    $exe = Find-WorkBuddyExe
    if (-not $exe) {
        Write-Step -Stage 'start' -Message '未找到 WorkBuddy 安装路径' -Status 'error'
        throw '未找到 WorkBuddy 可执行文件'
    }
    Write-Step -Stage 'start' -Message "正在启动 WorkBuddy: $exe" -Status 'running'
    Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent)
    Start-Sleep -Seconds 2
    Write-Step -Stage 'start' -Message 'WorkBuddy 已启动' -Status 'ok'
}

# ── 快照路径 ─────────────────────────────────────────────────────────────────
function Get-SlotAuthPath {
    param([string]$Slot)
    return Join-Path $Script:ProfilesDir (Join-Path $Slot 'auth\workbuddy-desktop.info')
}

# ── 保存当前登录态 ───────────────────────────────────────────────────────────
function Save-Profile {
    param([string]$Slot)
    $authFile = Get-AuthFile
    if (-not (Test-Path -LiteralPath $authFile)) {
        Write-Step -Stage 'backup' -Message "找不到 WorkBuddy 登录态文件: $authFile" -Status 'error'
        throw "登录态文件不存在"
    }
    if ((Test-Path -LiteralPath (Get-AuthMarkerPath -AuthFilePath $authFile))) {
        Write-Step -Stage 'backup' -Message '当前处于「已登出」状态（存在 .logged-out 标记），请先在 WorkBuddy 里登录再保存' -Status 'error'
        throw '当前未登录'
    }
    $info = Read-AuthInfo -Path $authFile
    if (-not $info -or -not $info.Uid) {
        Write-Step -Stage 'backup' -Message '登录态文件解析失败或缺少 account.uid，请确认 WorkBuddy 已登录' -Status 'error'
        throw '登录态解析失败'
    }

    # 映射校验：防止「当前登录 A，却保存到 B 那一行」
    $map = Get-AccountMap
    if ($map.ContainsKey($Slot)) {
        $expect = $map[$Slot]
        if ($expect.Uid -and $expect.Uid -ne $info.Uid) {
            $msg = "当前 WorkBuddy 登录的是「$($info.Nickname)」($($info.Uid))，但你请求保存到「$($expect.Nickname)」那一行。请先在该行对应账号登录后再保存。"
            Write-Step -Stage 'backup' -Message $msg -Status 'error'
            throw $msg
        }
    }

    $dst = Get-SlotAuthPath -Slot $Slot
    $dstDir = Split-Path $dst -Parent
    if (-not (Test-Path -LiteralPath $dstDir)) { New-Item -ItemType Directory -Path $dstDir -Force | Out-Null }
    Copy-Item -LiteralPath $authFile -Destination $dst -Force

    $hash = (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash
    $manifest = [ordered]@{
        slot             = $Slot
        savedAt          = (Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
        sourceAuthFile   = $authFile
        accountUid       = $info.Uid
        accountNickname  = $info.Nickname
        accountUin       = $info.Uin
        authDomain       = $info.Domain
        accessTokenLen   = $info.TokenLen
        refreshTokenLen  = $info.RefreshLen
        refreshExpiresAt = $info.RefreshExpiresAt
        sha256           = $hash
        fileSize         = (Get-Item -LiteralPath $dst).Length
    }
    $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path (Join-Path $Script:ProfilesDir $Slot) 'manifest.json') -Encoding UTF8
    Write-Step -Stage 'backup' -Message "已保存「$($info.Nickname)」的登录态到 $Slot（uid=$($info.Uid)）" -Status 'ok'
}

# ── 恢复登录态 ───────────────────────────────────────────────────────────────
function Restore-Profile {
    param([string]$Slot)
    $src = Get-SlotAuthPath -Slot $Slot
    if (-not (Test-Path -LiteralPath $src)) {
        Write-Step -Stage 'restore' -Message "账号 $Slot 还没有登录态快照，请先登录该账号并点「保存登录态」" -Status 'error'
        throw "账号 $Slot 无快照"
    }
    $snap = Read-AuthInfo -Path $src
    if (-not $snap -or -not $snap.Uid) {
        Write-Step -Stage 'restore' -Message "账号 $Slot 的快照文件损坏（无法解析）" -Status 'error'
        throw "快照损坏"
    }

    if ($DryRun) {
        Write-Step -Stage 'restore' -Message "[DryRun] 将把「$($snap.Nickname)」($($snap.Uid)) 的登录态写入 $(Get-AuthFile)" -Status 'info'
        return
    }

    $authFile = Get-AuthFile
    # 先把现有登录态留一份现场备份（防意外）
    if (Test-Path -LiteralPath $authFile) {
        $bakDir = Join-Path $Script:AppDataDir 'data\auth-rollback'
        if (-not (Test-Path -LiteralPath $bakDir)) { New-Item -ItemType Directory -Path $bakDir -Force | Out-Null }
        $stamp = (Get-Date -Format 'yyyyMMdd-HHmmss')
        Copy-Item -LiteralPath $authFile -Destination (Join-Path $bakDir "$stamp-preSwitch.info") -Force
        Write-Step -Stage 'restore' -Message "切换前现场已备份到 data\auth-rollback\$stamp-preSwitch.info" -Status 'info'
    }

    $txt = [System.IO.File]::ReadAllText($src, [System.Text.Encoding]::UTF8)
    Write-AuthFileAtomic -Path $authFile -Content $txt

    $ok = $false
    for ($i = 0; $i -lt 10; $i++) {
        $chk = Read-AuthInfo -Path $authFile
        if ($chk -and $chk.Uid -eq $snap.Uid) { $ok = $true; break }
        Start-Sleep -Milliseconds 200
    }
    if (-not $ok) {
        Write-Step -Stage 'restore' -Message "写入后校验失败：登录态文件里的 uid 不是 $($snap.Uid)" -Status 'error'
        throw "恢复校验失败"
    }
    Write-Step -Stage 'restore' -Message "已写入「$($snap.Nickname)」的登录态（uid=$($snap.Uid)，accessToken $($snap.TokenLen) 字符）" -Status 'ok'

    $null = Clear-LogoutMarker -AuthFilePath $authFile
}

# ── 各 Action 的信息输出 ─────────────────────────────────────────────────────
function Show-Paths {
    $authFile = Get-AuthFile
    Write-Step -Stage 'paths' -Message "WorkBuddy 数据目录: $Script:WbDataDir" -Status 'info'
    Write-Step -Stage 'paths' -Message "登录态快照目录: $Script:ProfilesDir" -Status 'info'
    Write-Step -Stage 'paths' -Message "★ 唯一会改写的文件（登录态）:" -Status 'info'
    Write-Step -Stage 'paths' -Message "    $authFile" -Status 'info'
    Write-Step -Stage 'paths' -Message "    $((Get-AuthMarkerPath -AuthFilePath $authFile))  （登出标记，切换时删除）" -Status 'info'
    Write-Step -Stage 'paths' -Message '除此之外一切都不动：tasks、sessions、projects、file-history、settings、storage、connectors、workbuddy.db、memory…' -Status 'info'
    $cur = Read-AuthInfo -Path $authFile
    if ($cur) {
        Write-Step -Stage 'paths' -Message "当前登录: $($cur.Nickname)  uid=$($cur.Uid)  uin=$($cur.Uin)" -Status 'info'
    } else {
        Write-Step -Stage 'paths' -Message '当前登录: （未能读取登录态文件）' -Status 'warn'
    }
    $marker = Get-AuthMarkerPath -AuthFilePath $authFile
    Write-Step -Stage 'paths' -Message ("登出标记存在: " + (Test-Path -LiteralPath $marker)) -Status 'info'
}

function Get-CurrentAccount {
    if (Test-Path -LiteralPath $Script:CurrentAccountFile) {
        try {
            $id = (Get-Content -LiteralPath $Script:CurrentAccountFile -Raw).Trim()
            if ($id) { return $id }
        } catch {}
    }
    return $null
}

function Set-CurrentAccount {
    param([string]$AccountId)
    try {
        $dir = Split-Path $Script:CurrentAccountFile
        if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
        Set-Content -LiteralPath $Script:CurrentAccountFile -Value $AccountId -NoNewline -Encoding UTF8
    } catch {}
}

function Show-Profiles {
    if (-not (Test-Path -LiteralPath $Script:ProfilesDir)) {
        Write-Step -Stage 'profiles' -Message '还没有任何 WorkBuddy 登录态快照' -Status 'info'
        return
    }
    $dirs = @(Get-ChildItem -LiteralPath $Script:ProfilesDir -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'wb-*' })
    Write-Step -Stage 'profiles' -Message "共 $($dirs.Count) 个账号槽位" -Status 'info'
    $map = Get-AccountMap
    foreach ($d in $dirs) {
        $mp = Join-Path $d.FullName 'manifest.json'
        $ap = Join-Path $d.FullName 'auth\workbuddy-desktop.info'
        $label = $d.Name
        if ($map.ContainsKey($d.Name)) { $label = "$($d.Name) [$($map[$d.Name].Nickname)]" }
        if ((Test-Path -LiteralPath $ap) -and (Test-Path -LiteralPath $mp)) {
            try {
                $m = Get-Content -LiteralPath $mp -Raw -Encoding UTF8 | ConvertFrom-Json
                Write-Step -Stage 'profiles' -Message "  ✓ $label  账号=$($m.accountNickname)  uid=$($m.accountUid)  存于 $($m.savedAt)" -Status 'info'
            } catch {
                Write-Step -Stage 'profiles' -Message "  ? $label  有快照但 manifest 读取失败" -Status 'warn'
            }
        } else {
            Write-Step -Stage 'profiles' -Message "  ✗ $label  尚无登录态快照（需先保存一次）" -Status 'warn'
        }
    }
    $cur = Get-CurrentAccount
    if ($cur) { Write-Step -Stage 'profiles' -Message "上次切换记录: $cur" -Status 'info' }
    $live = Read-AuthInfo -Path (Get-AuthFile)
    if ($live) { Write-Step -Stage 'profiles' -Message "客户端当前实际登录: $($live.Nickname) ($($live.Uid))" -Status 'info' }
}

function Show-Fingerprint {
    $authFile = Get-AuthFile
    $info = Read-AuthInfo -Path $authFile
    if ($info) {
        Write-Step -Stage 'fp' -Message "登录态文件: $authFile" -Status 'info'
        Write-Step -Stage 'fp' -Message "  nickname=$($info.Nickname) uid=$($info.Uid) uin=$($info.Uin) domain=$($info.Domain)" -Status 'info'
        Write-Step -Stage 'fp' -Message "  accessToken=$($info.TokenLen) 字符  refreshToken=$($info.RefreshLen) 字符" -Status 'info'
        Write-Step -Stage 'fp' -Message "  sha256=$((Get-FileHash -LiteralPath $authFile -Algorithm SHA256).Hash)" -Status 'info'
    } else {
        Write-Step -Stage 'fp' -Message '未能解析登录态文件' -Status 'warn'
    }
}

# ── 从现存 auth 备份里播种快照 ───────────────────────────────────────────────
function Seed-AuthSnapshots {
    $authFile = Get-AuthFile
    $dir = Split-Path $authFile -Parent
    if (-not (Test-Path -LiteralPath $dir)) {
        Write-Step -Stage 'seed' -Message "找不到登录态目录: $dir" -Status 'error'
        return
    }
    $map = Get-AccountMap
    if ($map.Count -eq 0) {
        Write-Step -Stage 'seed' -Message '读不到 workbuddy_accounts.json，无法建立 slot↔uid 映射' -Status 'error'
        return
    }
    # 收集所有候选：当前文件 + 历史备份（清理动作会留 <basename>.<iso>.<pid>.<uuid>.info）
    $cands = @()
    foreach ($f in (Get-ChildItem -LiteralPath $dir -File -Filter 'workbuddy-desktop*.info' -ErrorAction SilentlyContinue)) {
        if ($f.Name -like '*.tmp.*') { continue }
        $inf = Read-AuthInfo -Path $f.FullName
        if ($inf -and $inf.Uid) {
            $cands += [pscustomobject]@{ Path = $f.FullName; Name = $f.Name; Info = $inf; Mtime = $f.LastWriteTime; IsCurrent = ($f.Name -eq 'workbuddy-desktop.info') }
        }
    }
    if ($cands.Count -eq 0) {
        Write-Step -Stage 'seed' -Message '没有可用于播种的登录态文件' -Status 'warn'
        return
    }
    Write-Step -Stage 'seed' -Message "扫描到 $($cands.Count) 个登录态文件，按账号取最新的一个播种" -Status 'info'

    $byUid = @{}
    foreach ($c in $cands) {
        if (-not $byUid.ContainsKey($c.Info.Uid)) { $byUid[$c.Info.Uid] = $c; continue }
        $prev = $byUid[$c.Info.Uid]
        if ($c.IsCurrent -and -not $prev.IsCurrent) { $byUid[$c.Info.Uid] = $c }
        elseif ($c.IsCurrent -eq $prev.IsCurrent -and $c.Mtime -gt $prev.Mtime) { $byUid[$c.Info.Uid] = $c }
    }

    foreach ($uid in $byUid.Keys) {
        $c = $byUid[$uid]
        $slot = Get-SlotForUid -Uid $uid
        if (-not $slot) {
            Write-Step -Stage 'seed' -Message "  跳过 uid=$uid（$($c.Info.Nickname)）：workbuddy_accounts.json 里没有对应槽位" -Status 'warn'
            continue
        }
        $dst = Get-SlotAuthPath -Slot $slot
        $dstDir = Split-Path $dst -Parent
        if ($DryRun) {
            Write-Step -Stage 'seed' -Message "  [DryRun] $slot <- $($c.Name)（$($c.Info.Nickname)）" -Status 'info'
            continue
        }
        if (-not (Test-Path -LiteralPath $dstDir)) { New-Item -ItemType Directory -Path $dstDir -Force | Out-Null }
        Copy-Item -LiteralPath $c.Path -Destination $dst -Force
        $hash = (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash
        $manifest = [ordered]@{
            slot             = $slot
            savedAt          = (Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
            sourceAuthFile   = $c.Path
            seededFrom       = $c.Name
            accountUid       = $c.Info.Uid
            accountNickname  = $c.Info.Nickname
            accountUin       = $c.Info.Uin
            authDomain       = $c.Info.Domain
            accessTokenLen   = $c.Info.TokenLen
            refreshTokenLen  = $c.Info.RefreshLen
            refreshExpiresAt = $c.Info.RefreshExpiresAt
            sha256           = $hash
            fileSize         = (Get-Item -LiteralPath $dst).Length
        }
        $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path (Join-Path $Script:ProfilesDir $slot) 'manifest.json') -Encoding UTF8
        Write-Step -Stage 'seed' -Message "  ✓ $slot <- $($c.Info.Nickname)（来源 $($c.Name)，$($c.Mtime.ToString('MM-dd HH:mm'))）" -Status 'ok'
    }
}

# ============ 入口 ============
try {
    if ($Action -ne 'ShowPaths' -and $Action -ne 'ListProfiles' -and $Action -ne 'SeedAuthSnapshots' -and -not $UserId) {
        Write-Step -Stage 'init' -Message '缺少 -UserId 参数' -Status 'error'
        exit 1
    }
    if ($UserId -and $UserId -notlike 'wb-*') {
        Write-Step -Stage 'init' -Message "WorkBuddy 桥只接受 wb- 前缀的账号 ID，收到: $UserId" -Status 'error'
        exit 1
    }
    Write-Step -Stage 'init' -Message "开始 WorkBuddy 操作: $Action (accountId=$UserId)" -Status 'info'

    switch ($Action) {
        'ShowPaths' { Show-Paths; Write-Step -Stage 'done' -Message '路径清单输出完成' -Status 'ok' }
        'ListProfiles' { Show-Profiles; Write-Step -Stage 'done' -Message '快照清单输出完成' -Status 'ok' }
        'Fingerprint' { Show-Fingerprint; Write-Step -Stage 'done' -Message '指纹输出完成' -Status 'ok' }
        'SeedAuthSnapshots' { Seed-AuthSnapshots; Write-Step -Stage 'done' -Message '播种完成' -Status 'ok' }
        'BackupCurrent' {
            Save-Profile -Slot $UserId
            Set-CurrentAccount -AccountId $UserId
            Write-Step -Stage 'done' -Message '备份完成' -Status 'ok'
        }
        'SaveCurrentLogin' {
            if ($DryRun) {
                $a = Get-AuthFile
                Write-Step -Stage 'save' -Message "[DryRun] 将把 $(Read-AuthInfo -Path $a).Nickname 的登录态保存为 $UserId" -Status 'info'
                Write-Step -Stage 'done' -Message '[DryRun] 未做任何改动' -Status 'ok'
                exit 0
            }
            Stop-WorkBuddy
            Save-Profile -Slot $UserId
            Set-CurrentAccount -AccountId $UserId
            Start-WorkBuddy
            Write-Step -Stage 'done' -Message "已保存账号 $UserId 的登录态" -Status 'ok'
        }
        'RestoreOnly' {
            Stop-WorkBuddy
            Restore-Profile -Slot $UserId
            Set-CurrentAccount -AccountId $UserId
            Start-WorkBuddy
            Write-Step -Stage 'done' -Message "已恢复账号 $UserId 的登录态" -Status 'ok'
        }
        'Switch' {
            $src = Get-SlotAuthPath -Slot $UserId
            if (-not (Test-Path -LiteralPath $src)) {
                Write-Step -Stage 'fatal' -Message "账号 $UserId 还没有登录态快照：请先用该账号登录 WorkBuddy 客户端，再点该行的「保存登录态」按钮" -Status 'error'
                exit 1
            }
            if ($DryRun) {
                Write-Step -Stage 'switch' -Message "[DryRun] 将关闭 WorkBuddy → 写入 $UserId 的登录态 → 重新启动" -Status 'info'
                Restore-Profile -Slot $UserId
                Write-Step -Stage 'done' -Message '[DryRun] 未做任何改动' -Status 'ok'
                exit 0
            }
            Stop-WorkBuddy

            # 顺手把「当前真实登录」的现场刷新回它自己的槽位，保证随时能切回来
            $live = Read-AuthInfo -Path (Get-AuthFile)
            if ($live -and $live.Uid) {
                $liveSlot = Get-SlotForUid -Uid $live.Uid
                if ($liveSlot -and $liveSlot -ne $UserId) {
                    try {
                        Save-Profile -Slot $liveSlot
                        Write-Step -Stage 'backup' -Message "已自动刷新当前账号「$($live.Nickname)」的快照（$liveSlot）" -Status 'ok'
                    } catch {
                        Write-Step -Stage 'backup' -Message "刷新当前账号快照失败（不影响本次切换）: $_" -Status 'warn'
                    }
                }
            }

            Restore-Profile -Slot $UserId
            Set-CurrentAccount -AccountId $UserId
            Start-WorkBuddy
            $t = Read-AuthInfo -Path (Get-AuthFile)
            if ($t) {
                Write-Step -Stage 'done' -Message "已切换到「$($t.Nickname)」($($t.Uid))。任务与历史记录未做任何改动。" -Status 'ok'
            } else {
                Write-Step -Stage 'done' -Message "已切换到账号 $UserId（任务与历史记录未做任何改动）" -Status 'ok'
            }
        }
    }
    exit 0
} catch {
    Write-Step -Stage 'fatal' -Message "失败: $_" -Status 'error'
    exit 1
}
