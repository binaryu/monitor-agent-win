# PowerShell 脚本: 注册 Monitor Agent 为 Windows 开机自启后台任务 (免黑框窗口)
param (
    [string]$Server = "",
    [string]$Token = "",
    [int]$Interval = 1,
    [switch]$Insecure
)

# 确保以管理员权限运行
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "请以管理员身份运行 PowerShell 执行此安装脚本！"
    exit 1
}

$CurrentDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ExePath = Join-Path $CurrentDir "monitor-agent.exe"

if (-not (Test-Path $ExePath)) {
    Write-Error "未在当前目录找到 monitor-agent.exe: $ExePath"
    exit 1
}

# 检查/提示输入参数
if ([string]::IsNullOrWhiteSpace($Server)) {
    $Server = Read-Host "请输入 Monitor 服务端地址 (例如 https://hub.example.com)"
}
if ([string]::IsNullOrWhiteSpace($Token)) {
    $Token = Read-Host "请输入节点 Token"
}

if ([string]::IsNullOrWhiteSpace($Server) -or [string]::IsNullOrWhiteSpace($Token)) {
    Write-Error "Server 和 Token 不能为空！"
    exit 1
}

$Arguments = "--server `"$Server`" --token `"$Token`" --interval $Interval"
if ($Insecure) {
    $Arguments += " --insecure"
}

$TaskName = "MonitorAgent"

# 如果已存在旧任务则先注销
Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue

# 创建计划任务：开机自启、SYSTEM 权限、后台静默运行
$Action = New-ScheduledTaskAction -Execute $ExePath -Argument $Arguments -WorkingDirectory $CurrentDir
$Trigger = New-ScheduledTaskTrigger -AtStartup
$Principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest
$Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit 0

Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger -Principal $Principal -Settings $Settings | Out-Null
Start-ScheduledTask -TaskName $TaskName

Write-Host "`n[✓] Monitor Agent 已成功注册为 Windows 系统后台任务并立即启动！" -ForegroundColor Green
Write-Host "任务名称: $TaskName"
Write-Host "如需停止或卸载，可运行 .\uninstall-service.ps1"
