# PowerShell 一键在线安装与服务注册脚本 for Monitor Agent (Windows)
# 支持一行命令在线执行:
#   irm https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/install.ps1 | iex
# 或带参数:
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/install.ps1))) -Server "https://hub.example.com" -Token "xxx"

param (
    [string]$Server = "",
    [string]$Token = "",
    [int]$Interval = 1,
    [switch]$Insecure,
    [string]$InstallDir = "$env:ProgramFiles\MonitorAgent",
    [string]$Version = "latest"
)

# 1. 确保以管理员权限运行
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[!] 正在请求管理员权限以注册系统后台服务..." -ForegroundColor Yellow
    Start-Process powershell.exe -ArgumentList "-NoProfile -ExecutionPolicy Bypass -Command `"& ([scriptblock]::Create((irm https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/install.ps1))) -Server '$Server' -Token '$Token' -Interval $Interval $(if($Insecure){'-Insecure'})`"" -Verb RunAs
    exit
}

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "   Monitor Agent Windows 一键安装程序    " -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

# 2. 交互式输入参数（如果命令行未指定）
if ([string]::IsNullOrWhiteSpace($Server)) {
    $Server = Read-Host "`n[?] 请输入 Monitor 服务端地址 (例如 https://hub.example.com)"
}
if ([string]::IsNullOrWhiteSpace($Token)) {
    $Token = Read-Host "[?] 请输入节点 Token (从控制面板添加节点后获取)"
}

if ([string]::IsNullOrWhiteSpace($Server) -or [string]::IsNullOrWhiteSpace($Token)) {
    Write-Error "[x] 错误: Server 地址和 Token 不能为空！"
    exit 1
}

# 3. 架构检测 (x86_64 或 arm64)
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x86_64" }
$assetName = "monitor-agent-windows-$arch.zip"
Write-Host "`n[*] 检测到系统架构: $arch" -ForegroundColor Gray

# 4. 下载最新二进制文件
$repo = "binaryu/monitor-agent-win"
$zipUrl = if ($Version -eq "latest") {
    "https://github.com/$repo/releases/latest/download/$assetName"
} else {
    "https://github.com/$repo/releases/download/$Version/$assetName"
}

Write-Host "[*] 正在从 GitHub 下载最新版本..." -ForegroundColor Gray
Write-Host "    $zipUrl" -ForegroundColor DarkGray

$tempZip = Join-Path $env:TEMP $assetName
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
    Invoke-WebRequest -Uri $zipUrl -OutFile $tempZip -UseBasicParsing
} catch {
    Write-Error "[x] 下载失败: $_. 请检查网络连接或 GitHub 访问情况。"
    exit 1
}

# 5. 安装到目标目录
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# 停止已有的进程
Stop-Process -Name "monitor-agent" -Force -ErrorAction SilentlyContinue

Write-Host "[*] 正在解压并安装到: $InstallDir" -ForegroundColor Gray
Expand-Archive -Path $tempZip -DestinationPath $InstallDir -Force
Remove-Item $tempZip -Force -ErrorAction SilentlyContinue

$exePath = Join-Path $InstallDir "monitor-agent.exe"
if (-not (Test-Path $exePath)) {
    Write-Error "[x] 安装目录中未找到 monitor-agent.exe！"
    exit 1
}

# 6. 注册 Windows 计划任务（开机自启、SYSTEM 权限、后台静默运行免窗口黑框）
$TaskName = "MonitorAgent"
Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue

$Arguments = "--server `"$Server`" --token `"$Token`" --interval $Interval"
if ($Insecure) {
    $Arguments += " --insecure"
}

$Action = New-ScheduledTaskAction -Execute $exePath -Argument $Arguments -WorkingDirectory $InstallDir
$Trigger = New-ScheduledTaskTrigger -AtStartup
$Principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest
$Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit 0

Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger -Principal $Principal -Settings $Settings | Out-Null
Start-ScheduledTask -TaskName $TaskName

Write-Host "`n==========================================" -ForegroundColor Green
Write-Host "[✓] Monitor Agent 安装成功并已作为后台服务启动！" -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Green
Write-Host "安装路径: $InstallDir"
Write-Host "后台任务: $TaskName"
Write-Host "服务器:   $Server"
Write-Host "`n如需卸载，可在管理员 PowerShell 中运行:" -ForegroundColor Gray
Write-Host "  irm https://raw.githubusercontent.com/$repo/main/uninstall.ps1 | iex" -ForegroundColor Yellow
