# PowerShell 离线安装与服务注册脚本
param (
    [string]$Server = "",
    [string]$Token = "",
    [int]$Interval = 1,
    [switch]$Insecure
)

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "请以管理员身份运行 PowerShell 执行此脚本！"
    exit 1
}

$CurrentDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ExePath = Join-Path $CurrentDir "monitor-agent.exe"

if (-not (Test-Path $ExePath)) {
    Write-Error "未在当前目录找到 monitor-agent.exe: $ExePath"
    exit 1
}

while ([string]::IsNullOrWhiteSpace($Server)) {
    $Server = Read-Host "请输入 Monitor 服务端地址 (例如 https://hub.example.com)"
    $Server = $Server.Trim()
}
while ([string]::IsNullOrWhiteSpace($Token)) {
    $Token = Read-Host "请输入节点 Token"
    $Token = $Token.Trim()
}

$Arguments = "--server `"$Server`" --token `"$Token`" --interval $Interval"
if ($Insecure) {
    $Arguments += " --insecure"
}

$TaskName = "MonitorAgent"

Stop-Process -Name "monitor-agent" -Force -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue

$Action = New-ScheduledTaskAction -Execute $ExePath -Argument $Arguments -WorkingDirectory $CurrentDir
$Trigger = New-ScheduledTaskTrigger -AtStartup
$Principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest
$Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit 0 -StartWhenAvailable

Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger -Principal $Principal -Settings $Settings | Out-Null
Start-ScheduledTask -TaskName $TaskName

Start-Sleep -Seconds 2
$proc = Get-Process -Name "monitor-agent" -ErrorAction SilentlyContinue

if ($proc) {
    Write-Host "`n[✓] Monitor Agent 已成功注册为 Windows 系统后台任务并立即运行！" -ForegroundColor Green
    Write-Host "进程 PID: $($proc.Id)"
    Write-Host "日志文件: $CurrentDir\agent.log"
} else {
    Write-Host "`n[!] 任务已创建，请检查 $CurrentDir\agent.log 查看日志。" -ForegroundColor Yellow
}
