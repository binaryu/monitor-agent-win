# PowerShell 脚本: 停止并卸载 Monitor Agent 计划任务
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "请以管理员身份运行 PowerShell 执行此卸载脚本！"
    exit 1
}

$TaskName = "MonitorAgent"

try {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop
    Stop-Process -Name "monitor-agent" -Force -ErrorAction SilentlyContinue
    Write-Host "[✓] 已成功停止并删除 $TaskName 任务！" -ForegroundColor Green
} catch {
    Write-Warning "未找到正在运行的 $TaskName 任务或已卸载。"
}
