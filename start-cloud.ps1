#Requires -Version 5.1
<#
.SYNOPSIS
    SuperTask 云服务端（后端）+ 云管理控制台（前端）一键启动。

.DESCRIPTION
    拉起两个独立窗口的进程，并等两者就绪后打开浏览器：
      云服务端   cargo run -p supertask-cloud-server   http://127.0.0.1:8787  （/admin/api/* 管理面）
      云控制台   npm run console:dev                   http://127.0.0.1:1430/admin/  （vite 代理 /admin/api → 8787）

    管理员邮箱与口令不落盘、不写进本文件：留空则交互输入（口令不回显）；
    若当前 shell 已注入 SUPERTASK_ADMIN_EMAIL / SUPERTASK_ADMIN_PASSWORD，脚本直接复用。
    服务端禁止默认口令，口令不足 12 字符会在配置解析阶段拒绝启动，所以脚本先本地校验。

    只想看成品界面而不改前端：先 `npm run build:console`，服务端会在 8787/admin/ 直接托管，
    不需要控制台 dev 窗口。

    脚本会一直占住当前终端，直到某个服务窗口被关闭（随后清理另一棵进程树）。服务报错退出时，
    它的窗口停在「请按任意键继续」，日志留在窗口里可读；按键关掉该窗口即触发整体清理。
    仅 Windows（依赖 cmd / taskkill / Start-Process 新窗口）。

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\start-cloud.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\start-cloud.ps1 -AdminEmail ops@example.com -NoBrowser
#>
[CmdletBinding()]
param(
    [string] $AdminEmail = $env:SUPERTASK_ADMIN_EMAIL,
    [string] $ServerBind = '127.0.0.1:8787',
    [switch] $NoBrowser
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$serverUrl = 'http://' + $ServerBind
$consoleUrl = 'http://127.0.0.1:1430/admin/'

function Start-ServiceWindow {
    param([string] $Title, [string] $Command, [string] $WorkDir)
    # 必须经 cmd 拉起：cargo.exe 是 rustup shim，直接在新控制台窗口启动会以 code 1 退出。
    # 结尾 pause 让报错退出的服务把日志留在窗口里，而不是闪退看不到原因。
    return Start-Process -FilePath 'cmd.exe' `
        -ArgumentList '/c', ('title ' + $Title + ' & ' + $Command + ' & echo. & pause') `
        -WorkingDirectory $WorkDir -PassThru
}

function Test-HttpReady {
    param([string] $Url)
    try {
        $null = Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 2 -ErrorAction Stop
        return $true
    } catch {
        return $false
    }
}

function Wait-HttpReady {
    param([string] $Url, [string] $What, [int] $TimeoutSec, $Watch)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while (-not (Test-HttpReady -Url $Url)) {
        if ($Watch -and $Watch.HasExited) {
            throw "$What 窗口已退出（code $($Watch.ExitCode)）：cmd 没能把服务拉起来，请检查 cargo / npm 是否在 PATH。"
        }
        if ((Get-Date) -gt $deadline) {
            throw "$What 在 $TimeoutSec 秒内没有就绪：请看它的窗口（服务报错时会停在「请按任意键继续」）。云控制台端口固定 1430，被占用时 vite 会直接退出。"
        }
        Start-Sleep -Seconds 2
    }
}

# ---- 预检：工具链、端口、输入 ----
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'PATH 里找不到 cargo：先安装 Rust 工具链。' }
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { throw 'PATH 里找不到 npm：先安装 Node.js。' }
if (Test-HttpReady -Url "$serverUrl/healthz") {
    throw "$serverUrl 已有云服务端在运行：关掉那个窗口，或执行 taskkill /IM supertask-cloud-server.exe /F"
}

if ([string]::IsNullOrWhiteSpace($AdminEmail)) { $AdminEmail = Read-Host -Prompt '管理员邮箱' }
if ([string]::IsNullOrWhiteSpace($AdminEmail) -or $AdminEmail -notmatch '^[^@\s]+@[^@\s]+\.[^@\s]+$') {
    throw '管理员邮箱格式无效（需要 a@b.tld 形式）。'
}

$adminPassword = $env:SUPERTASK_ADMIN_PASSWORD
if ([string]::IsNullOrWhiteSpace($adminPassword)) {
    $secure = Read-Host -Prompt '管理员口令（至少 12 字符，输入不回显）' -AsSecureString
    $adminPassword = [pscredential]::new('unused', $secure).GetNetworkCredential().Password
}
if ($adminPassword.Length -lt 12) { throw '管理员口令不足 12 个字符：服务端会直接拒绝启动。' }

if (-not (Test-Path (Join-Path $root 'cloud-console\node_modules'))) {
    Write-Host '首次运行：安装 cloud-console 依赖 ...'
    Push-Location (Join-Path $root 'cloud-console')
    try {
        npm install
        if ($LASTEXITCODE -ne 0) { throw 'cloud-console 依赖安装失败。' }
    } finally { Pop-Location }
}

# ---- 运行时注入（只在本进程与其子进程可见，脚本退出前清掉口令）----
$env:SUPERTASK_BIND = $ServerBind
$env:SUPERTASK_ADMIN_EMAIL = $AdminEmail.Trim().ToLowerInvariant()
$env:SUPERTASK_ADMIN_PASSWORD = $adminPassword
# vite 的代理目标，保证控制台 dev 打到本脚本启的这个端口
$env:SUPERTASK_API_TARGET = $serverUrl

$server = $null
$console = $null
try {
    $server = Start-ServiceWindow -Title 'SuperTask Cloud Server' `
        -Command 'cargo run -p supertask-cloud-server' -WorkDir $root
    Write-Host "云服务端启动中（首次要编译，可能要几分钟）... $serverUrl"
    Wait-HttpReady -Url "$serverUrl/healthz" -What '云服务端' -TimeoutSec 600 -Watch $server

    $status = Invoke-RestMethod -Uri "$serverUrl/admin/api/status" -TimeoutSec 5
    Write-Host "云服务端就绪：admin_available=$($status.admin_available) console_ready=$($status.console_ready)"
    if (-not $status.console_ready) {
        Write-Host '  提示：8787/admin/ 目前没有构建产物（会显示构建提示页）；改前端请用下面的 dev 窗口。'
    }

    $console = Start-ServiceWindow -Title 'SuperTask Cloud Console' `
        -Command 'npm run console:dev' -WorkDir $root
    Write-Host "云控制台 dev 启动中 ... $consoleUrl"
    Wait-HttpReady -Url $consoleUrl -What '云控制台' -TimeoutSec 120 -Watch $console

    if (-not $NoBrowser) { Start-Process $consoleUrl }
    Write-Host ''
    Write-Host "已就绪：登录入口 $consoleUrl ；管理员 $($env:SUPERTASK_ADMIN_EMAIL) ；服务端 $serverUrl"
    Write-Host '关闭任一服务窗口即整体停止（另一个会被一并清理）；在本窗口按 Ctrl+C 效果相同。'

    while (-not $server.HasExited -and -not $console.HasExited) { Start-Sleep -Milliseconds 500 }
} finally {
    foreach ($child in @($server, $console)) {
        if ($child -and -not $child.HasExited) {
            # /T 清整棵子进程树：cmd → npm → node，cmd → cargo → supertask-cloud-server
            & taskkill.exe '/T', '/F', '/PID', $child.Id 2>$null | Out-Null
        }
    }
    Remove-Item 'Env:SUPERTASK_ADMIN_PASSWORD' -ErrorAction SilentlyContinue
    Write-Host '本脚本拉起的进程已清理。'
}
