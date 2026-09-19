# PowerShell 一键在线安装与服务注册脚本 for Monitor Agent (Windows)
param (
    [string]$Server = "",
    [string]$Token = "",
    [int]$Interval = 1,
    [switch]$Insecure,
    [string]$Proxy = "https://gh-proxy.com",
    [string]$InstallDir = "$env:ProgramFiles\MonitorAgent",
    [string]$Version = "latest"
)

# 1. 确保以管理员权限运行
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[!] 需要管理员权限以注册后台服务，正在提升权限..." -ForegroundColor Yellow
    $argsList = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`""
    if ($Server) { $argsList += " -Server `"$Server`"" }
    if ($Token) { $argsList += " -Token `"$Token`"" }
    if ($Interval) { $argsList += " -Interval $Interval" }
    if ($Insecure) { $argsList += " -Insecure" }
    if ($Proxy) { $argsList += " -Proxy `"$Proxy`"" }
    
    Start-Process powershell.exe -ArgumentList $argsList -Verb RunAs
    exit
}

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "   Monitor Agent Windows 一键安装程序    " -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

# 2. 交互式输入参数（如果未传参）
while ([string]::IsNullOrWhiteSpace($Server)) {
    $Server = Read-Host "`n[?] 请输入 Monitor 服务端地址 (例如 https://hub.example.com)"
    $Server = $Server.Trim()
}
while ([string]::IsNullOrWhiteSpace($Token)) {
    $Token = Read-Host "[?] 请输入节点 Token (从控制面板添加节点后获取)"
    $Token = $Token.Trim()
}

# 3. 架构检测 (x86_64 或 arm64)
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x86_64" }
$assetName = "monitor-agent-windows-$arch.zip"
Write-Host "`n[*] 系统架构: $arch" -ForegroundColor Gray

# 4. 下载逻辑（首选代理加速下载）
$repo = "binaryu/monitor-agent-win"
$rawZipUrl = if ($Version -eq "latest") {
    "https://github.com/$repo/releases/latest/download/$assetName"
} else {
    "https://github.com/$repo/releases/download/$Version/$assetName"
}

# 第一候选直接就是带 gh-proxy.com 加速前缀的 URL
$proxyPrefix = if ([string]::IsNullOrWhiteSpace($Proxy)) { "https://gh-proxy.com/" } else { $Proxy.TrimEnd('/') + '/' }

$downloadUrls = @(
    "$proxyPrefix$rawZipUrl",
    "https://gh-proxy.com/$rawZipUrl",
    "https://mirror.ghproxy.com/$rawZipUrl",
    "https://ghproxy.net/$rawZipUrl",
    $rawZipUrl
)

$tempZip = Join-Path $env:TEMP $assetName
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13

$downloadSuccess = $false
foreach ($url in $downloadUrls) {
    try {
        Write-Host "[*] 正在通过加速源下载: $url" -ForegroundColor Gray
        Invoke-WebRequest -Uri $url -OutFile $tempZip -UseBasicParsing -TimeoutSec 20
        if ((Test-Path $tempZip) -and ((Get-Item $tempZip).Length -gt 1000)) {
            $downloadSuccess = $true
            Write-Host "[✓] 下载完成！" -ForegroundColor Green
            break
        }
    } catch {
        Write-Host "[!] 当前线路超时或失败，正在尝试备用线路..." -ForegroundColor DarkYellow
    }
}

if (-not $downloadSuccess) {
    Write-Error "[x] 所有下载线路均失败，请检查网络！"
    Read-Host "按回车键退出..."
    exit 1
}

# 5. 停止旧进程并解压安装到目标目录
Stop-Process -Name "monitor-agent" -Force -ErrorAction SilentlyContinue

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

Write-Host "[*] 正在解压安装到: $InstallDir" -ForegroundColor Gray
Expand-Archive -Path $tempZip -DestinationPath $InstallDir -Force
Remove-Item $tempZip -Force -ErrorAction SilentlyContinue

$exePath = Join-Path $InstallDir "monitor-agent.exe"
if (-not (Test-Path $exePath)) {
    Write-Error "[x] 错误: 安装目录中未找到 monitor-agent.exe！"
    Read-Host "按回车键退出..."
    exit 1
}

# 6. 注册 Windows 计划任务（开机自启、SYSTEM 权限、后台静默运行）
$TaskName = "MonitorAgent"
Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue

$Arguments = "--server `"$Server`" --token `"$Token`" --interval $Interval"
if ($Insecure) {
    $Arguments += " --insecure"
}

$Action = New-ScheduledTaskAction -Execute $exePath -Argument $Arguments -WorkingDirectory $InstallDir
$Trigger = New-ScheduledTaskTrigger -AtStartup
$Principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest
$Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit 0 -StartWhenAvailable

Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger -Principal $Principal -Settings $Settings | Out-Null
Start-ScheduledTask -TaskName $TaskName

# 等待 2 秒检查运行状态
Start-Sleep -Seconds 2
$proc = Get-Process -Name "monitor-agent" -ErrorAction SilentlyContinue

if ($proc) {
    Write-Host "`n==========================================" -ForegroundColor Green
    Write-Host "[✓] Monitor Agent 已成功启动并在后台正常运行！" -ForegroundColor Green
    Write-Host "==========================================" -ForegroundColor Green
    Write-Host "进程 PID: $($proc.Id)"
    Write-Host "安装路径: $InstallDir"
    Write-Host "日志文件: $InstallDir\agent.log"
} else {
    Write-Host "`n[!] 计划任务已创建，请检查日志: $InstallDir\agent.log" -ForegroundColor Yellow
}

Write-Host "`n[提示] 如需卸载，可随时运行:" -ForegroundColor Gray
Write-Host "  irm https://gh-proxy.com/https://raw.githubusercontent.com/$repo/main/uninstall.ps1 | iex" -ForegroundColor Yellow

if (-not $env:CI) {
    Start-Sleep -Seconds 3
}
