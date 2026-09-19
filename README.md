# Monitor Agent (Windows & 跨平台支持版)

针对 [monitor-probe/monitor](https://github.com/monitor-probe/monitor) 服务端开发的 Windows & 跨平台监控客户端（Agent）。

本仓库通过 `sysinfo` + `netstat2` 对硬件指标层进行了跨平台抽象，完整支持 **Windows 10/11 / Windows Server** 及 Linux，并与官方 Monitor 协议 100% 兼容。

---

## ✨ 特性

- 🪟 **原生 Windows 支持**：适配 Windows 10/11/Server (x86_64, ARM64)。
- 🚀 **超轻量极速**：单二进制文件（< 3MB），内存占用 < 10MB，极低 CPU 消耗。
- 🔄 **协议 100% 兼容**：完全支持官方 `hello` 静态硬件握手、`report` 周期指标上报、`ping.tasks` TCP 测速。
- 🛡️ **健壮连接**：内置 WebSocket 指数退避重连与心跳保活机制。
- 🤖 **CI/CD 全自动化**：GitHub Actions 自动交叉编译生成 Windows/Linux 构件并发布 Release。

---

## 📦 下载与安装

前往 [Releases 页面](../../releases) 下载对应系统的预编译包（例如 `monitor-agent-windows-x86_64.zip`）。

### 方式 1：一键注册为 Windows 系统后台自启服务（推荐）

解压后，在当前目录打开 **管理员权限** 的 PowerShell，运行：

```powershell
.\install-service.ps1 -Server "https://your-hub.example.com" -Token "YOUR_NODE_TOKEN"
```

> **参数说明**：
> - `-Server`: Monitor 仪表盘地址（支持 `https://...` 或 `http://...`）
> - `-Token`: 节点 Token
> - `-Interval`: 上报周期秒数（可选，默认 `1`）
> - `-Insecure`: 若服务端未配置 TLS/SSL 证书，可加此参数允许 `ws://`

如需卸载服务：
```powershell
.\uninstall-service.ps1
```

---

### 方式 2：命令行直接运行

在终端中执行：

```powershell
.\monitor-agent.exe --server https://your-hub.example.com --token YOUR_NODE_TOKEN
```

#### 命令行参数

| 参数 | 说明 | 示例 / 默认值 |
| :--- | :--- | :--- |
| `--server <url>` | Monitor Hub 地址 | `https://hub.example.com` |
| `--token <token>` | 节点认证 Token | `abc123xxx...` |
| `--interval <secs>`| 上报频率（秒） | 默认 `1` 秒 |
| `--insecure` | 允许非 TLS 的裸 HTTP/WS 远程连接 | 默认关闭 |
| `-h, --help` | 查看帮助文档 | |

也可以使用环境变量指定：
```powershell
$env:MONITOR_SERVER="https://hub.example.com"
$env:MONITOR_TOKEN="YOUR_NODE_TOKEN"
.\monitor-agent.exe
```

---

## 🛠️ 本地编译构建

要求 Rust 1.75+ 环境：

```bash
# 编译 Release 版本
cargo build --release

# 编译好的二进制位于 target/release/monitor-agent.exe (或 Linux 下的 monitor-agent)
```

---

## 📄 License

MIT License
