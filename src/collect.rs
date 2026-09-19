//! Cross-platform system metrics collection for monitor-agent.

use std::net::IpAddr;
use std::time::Instant;

use serde::Serialize;
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Facts {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub virt: String,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub mem_total: u64,
    pub swap_total: u64,
    pub disk_total: u64,
    pub agent_version: String,
    pub ipv4: String,
    pub ipv6: String,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct Metrics {
    pub boot_id: String,
    pub uptime: u64,
    pub cpu: f32,
    pub load: [f32; 3],
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub net_rx_total: u64,
    pub net_tx_total: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub tcp: u32,
    pub udp: u32,
    pub procs: u32,
}

pub struct Collector {
    sys: System,
    disks: Disks,
    networks: Networks,
    prev_net: Option<(Instant, u64, u64)>,
    boot_id: String,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::everything()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();

        let boot_time = System::boot_time();
        let boot_id = format!("boot-{}", boot_time);

        Self {
            sys,
            disks,
            networks,
            prev_net: None,
            boot_id,
        }
    }

    pub fn facts(&self) -> Facts {
        let (v4, v6) = addresses();
        let cpu_name = self
            .sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_else(|| "Unknown CPU".into());
        let cpu_cores = self.sys.cpus().len() as u32;

        let disk_total: u64 = self.disks.iter().map(|d| d.total_space()).sum();

        Facts {
            hostname: System::host_name().unwrap_or_else(|| "Host".into()),
            os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
            kernel: System::kernel_version().unwrap_or_else(|| "Unknown".into()),
            arch: std::env::consts::ARCH.into(),
            virt: detect_virt(),
            cpu_name,
            cpu_cores,
            mem_total: self.sys.total_memory(),
            swap_total: self.sys.total_swap(),
            disk_total,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            ipv4: v4,
            ipv6: v6,
        }
    }

    pub fn collect(&mut self) -> Metrics {
        self.sys.refresh_all();
        self.disks.refresh(true);
        self.networks.refresh(true);

        let cpu = self.sys.global_cpu_usage();
        let mem_total = self.sys.total_memory();
        let mem_used = self.sys.used_memory();
        let swap_total = self.sys.total_swap();
        let swap_used = self.sys.used_swap();

        let mut disk_total = 0u64;
        let mut disk_used = 0u64;
        for disk in &self.disks {
            let total = disk.total_space();
            let free = disk.available_space();
            disk_total += total;
            disk_used += total.saturating_sub(free);
        }

        let mut rx_total = 0u64;
        let mut tx_total = 0u64;
        for (iface, data) in &self.networks {
            // Filter virtual / tunnel networks if needed
            if is_virtual_iface(iface) {
                continue;
            }
            rx_total += data.total_received();
            tx_total += data.total_transmitted();
        }

        let (rx, tx) = self.net_rate(rx_total, tx_total, Instant::now());
        let (tcp, udp) = conn_counts();
        let procs = self.sys.processes().len() as u32;

        let load = [
            (cpu / 100.0).clamp(0.0, 100.0),
            (cpu / 100.0).clamp(0.0, 100.0),
            (cpu / 100.0).clamp(0.0, 100.0),
        ];

        Metrics {
            boot_id: self.boot_id.clone(),
            uptime: System::uptime(),
            cpu,
            load,
            mem_total,
            mem_used,
            swap_total,
            swap_used,
            disk_total,
            disk_used,
            net_rx_total: rx_total,
            net_tx_total: tx_total,
            net_rx: rx,
            net_tx: tx,
            tcp,
            udp,
            procs,
        }
    }

    fn net_rate(&mut self, rx_total: u64, tx_total: u64, now: Instant) -> (u64, u64) {
        let Some((prev_time, prev_rx, prev_tx)) = self.prev_net else {
            self.prev_net = Some((now, rx_total, tx_total));
            return (0, 0);
        };
        let elapsed = now.duration_since(prev_time).as_secs_f64().max(0.001);
        let rx_rate = ((rx_total.saturating_sub(prev_rx) as f64) / elapsed) as u64;
        let tx_rate = ((tx_total.saturating_sub(prev_tx) as f64) / elapsed) as u64;
        self.prev_net = Some((now, rx_total, tx_total));
        (rx_rate, tx_rate)
    }
}

fn detect_virt() -> String {
    // Under Windows, simple heuristic or "none"
    #[cfg(target_os = "windows")]
    {
        "none".into()
    }
    #[cfg(not(target_os = "windows"))]
    {
        if std::path::Path::new("/run/systemd/container").exists() {
            "container".into()
        } else {
            "none".into()
        }
    }
}

fn is_virtual_iface(name: &str) -> bool {
    let lower = name.to_lowercase();
    let skip = [
        "loopback", "isatap", "teredo", "vethernet", "docker", "veth", "br-", "virbr", "tap",
        "tun", "wg", "tailscale", "cni", "flannel", "podman", "zt",
    ];
    skip.iter().any(|&s| lower.contains(s))
}

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || a == 0
                || a >= 224
                || (a == 100 && b & 0xc0 == 64)
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && b & 0xfe == 18))
        }
        IpAddr::V6(v6) => v6.segments()[0] & 0xe000 == 0x2000,
    }
}

fn addresses() -> (String, String) {
    let mut v4s = Vec::new();
    let mut v6s = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() || is_virtual_iface(&iface.name) {
                continue;
            }
            match iface.ip() {
                IpAddr::V4(v4) => v4s.push(IpAddr::V4(v4)),
                IpAddr::V6(v6) => v6s.push(IpAddr::V6(v6)),
            }
        }
    }
    v4s.sort_by_key(|ip| !is_public(*ip));
    v6s.sort_by_key(|ip| !is_public(*ip));
    (
        v4s.first().map(|ip| ip.to_string()).unwrap_or_default(),
        v6s.first().map(|ip| ip.to_string()).unwrap_or_default(),
    )
}

fn conn_counts() -> (u32, u32) {
    let af_flags = netstat2::AddressFamilyFlags::IPV4 | netstat2::AddressFamilyFlags::IPV6;
    let proto_flags = netstat2::ProtocolFlags::TCP | netstat2::ProtocolFlags::UDP;
    let mut tcp = 0;
    let mut udp = 0;
    if let Ok(sockets) = netstat2::get_sockets_info(af_flags, proto_flags) {
        for s in sockets {
            match s.protocol_socket_info {
                netstat2::ProtocolSocketInfo::Tcp(_) => tcp += 1,
                netstat2::ProtocolSocketInfo::Udp(_) => udp += 1,
            }
        }
    }
    (tcp, udp)
}

#[allow(dead_code)]
pub fn shadowed_mounts(_: &str) -> Vec<String> {
    Vec::new()
}
