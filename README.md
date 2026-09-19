# Monitor Agent (Windows & 跨平台支持版)

针对 [monitor-probe/monitor](https://github.com/monitor-probe/monitor) 服务端开发的 Windows & 跨平台监控客户端（Agent）。

本仓库通过 `sysinfo` + `netstat2` 对硬件指标层进行了跨平台抽象，完整支持 **Windows 10/11 / Windows Server (x86_64, ARM64)** 及 Linux，与官方 Monitor 服务端协议 100% 兼容。

---

## ⚡ 一键安装（推荐）

在 Windows 电脑上打开 **PowerShell**，复制并执行以下命令即可全自动安装：

### 1. 默认安装
```powershell
irm https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/install.ps1 | iex
```

### 2. 国内网络加速安装（推荐国内网络）
若 GitHub 直连慢或下载失败，可直接使用加速代理通道：
```powershell
irm https://gh-proxy.com/https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/install.ps1 | iex
```

### 3. 带参数静默安装（支持指定代理与服务端参数）
```powershell
& ([scriptblock]::Create((irm https://gh-proxy.com/https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/install.ps1))) -Server "https://hub.example.com" -Token "YOUR_NODE_TOKEN" -Proxy "https://gh-proxy.com"
```

> **可选参数**：
> - `-Server`: Monitor 仪表盘地址（如 `https://hub.example.com`）
> - `-Token`: 节点 Token
> - `-Interval`: 上报周期秒数（默认 `1`）
> - `-Proxy`: 指定 GitHub 下载加速镜像（默认内置多节点自动重试故障转移）
> - `-Insecure`: 若服务端为纯 HTTP/WS（未配置 SSL 证书）时加此开关

---

## 🗑️ 一键卸载

以管理员身份运行 PowerShell：
```powershell
irm https://raw.githubusercontent.com/binaryu/monitor-agent-win/main/uninstall.ps1 | iex
```

---

## ✨ 核心特性

- 🪟 **原生 Windows 支持**：适配 Windows 10/11/Server (x86_64, ARM64)。
- 🚀 **超轻量极速**：单二进制文件（< 3MB），内存占用 < 10MB，极低 CPU 消耗。
- 🔄 **协议 100% 兼容**：完全支持官方 `hello` 静态硬件握手、`report` 周期指标上报、`ping.tasks` TCP 测速。
- 🛡️ **健壮连接**：内置 WebSocket 指数退避重连与心跳保活机制。
- 🤖 **CI/CD 全自动化**：GitHub Actions 自动交叉编译生成 Windows/Linux 构件并发布 Release。

---

## 📦 手动运行方式（免安装）

前往 [Releases 页面](../../releases) 下载对应系统的预编译包（例如 `monitor-agent-windows-x86_64.zip`）。

解压后直接在终端中运行：
```powershell
.\monitor-agent.exe --server https://hub.example.com --token YOUR_NODE_TOKEN
```

---

## 🛠️ 本地编译构建

要求 Rust 1.75+ 环境：

```bash
# 编译 Release 版本
cargo build --release
```

---

## 📄 License

MIT License
