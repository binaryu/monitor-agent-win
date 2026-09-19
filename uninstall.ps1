# PowerShell 一键卸载脚本 for Monitor Agent (Windows)
# 运行方式:
#   irm https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/uninstall.ps1 | iex

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[!] 正在请求管理员权限以卸载服务..." -ForegroundColor Yellow
    Start-Process powershell.exe -ArgumentList "-NoProfile -ExecutionPolicy Bypass -Command `"& ([scriptblock]::Create((irm https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/uninstall.ps1)))`"" -Verb RunAs
    exit
}

Write-Host "[*] 正在停止并卸载 Monitor Agent..." -ForegroundColor Cyan

$TaskName = "MonitorAgent"
$InstallDir = "$env:ProgramFiles\MonitorAgent"

# 1. 停止并注销计划任务
Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue

# 2. 终止进程
Stop-Process -Name "monitor-agent" -Force -ErrorAction SilentlyContinue

# 3. 清理安装目录
if (Test-Path $InstallDir) {
    Start-Sleep -Seconds 1
    Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "[✓] Monitor Agent 已完全停止并卸载干净！" -ForegroundColor Green
