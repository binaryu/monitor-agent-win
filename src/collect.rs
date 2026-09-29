//! Cross-platform system metrics collection for monitor-agent.

use std::collections::HashMap;
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
    pub iface: String,
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

/// The traffic filter set by `--iface`: full interface names separated by commas.
///
/// An entry starting with `-` removes that interface from what is counted otherwise.
/// Full names only; exclusions win over inclusions.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Ifaces {
    pub spec: String,
    pub only: Vec<String>,
    pub skip: Vec<String>,
}

impl Ifaces {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let entries: Vec<&str> = spec.split(',').map(str::trim).filter(|e| !e.is_empty()).collect();
        let mut ifaces = Self {
            spec: entries.join(","),
            ..Self::default()
        };
        for entry in entries {
            let (list, name) = match entry.strip_prefix('-') {
                Some(name) => (&mut ifaces.skip, name.trim()),
                None => (&mut ifaces.only, entry),
            };
            if name.is_empty() || name.starts_with('-') || name.contains('\n') {
                return Err(format!(
                    "--iface: {entry:?} is not an interface name; give full names separated by commas"
                ));
            }
            list.push(name.to_owned());
        }
        Ok(ifaces)
    }

    pub fn counts(&self, name: &str) -> bool {
        if self.skip.iter().any(|n| n == name || n.eq_ignore_ascii_case(name)) {
            return false;
        }
        if !self.only.is_empty() {
            return self.only.iter().any(|n| n == name || n.eq_ignore_ascii_case(name));
        }
        !is_virtual_iface(name)
    }
}

/// Computes the composite boot_id: raw boot id plus FNV-1a digest of sorted interface names.
fn epoch<'a>(boot_id: &str, names: impl Iterator<Item = &'a str>) -> String {
    let mut names: Vec<&str> = names.collect();
    names.sort_unstable();
    let digest = names
        .join("\n")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{boot_id}/{digest:016x}")
}

pub struct Collector {
    sys: System,
    disks: Disks,
    networks: Networks,
    ifaces: Ifaces,
    prev_net_at: Option<Instant>,
    prev_net: HashMap<String, (u64, u64)>,
    raw_boot_id: String,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new(Ifaces::default())
    }
}

impl Collector {
    pub fn new(ifaces: Ifaces) -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::everything()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();

        let boot_time = System::boot_time();
        let raw_boot_id = format!("boot-{}", boot_time);

        Self {
            sys,
            disks,
            networks,
            ifaces,
            prev_net_at: None,
            prev_net: HashMap::new(),
            raw_boot_id,
        }
    }

    /// The interfaces the traffic totals include at this moment.
    pub fn counted_ifaces(&self) -> Vec<String> {
        let mut list: Vec<String> = self
            .networks
            .iter()
            .filter(|(name, _)| self.ifaces.counts(name))
            .map(|(name, _)| (*name).clone())
            .collect();
        list.sort();
        list
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

        let counted: Vec<(String, u64, u64)> = self
            .networks
            .iter()
            .filter(|(name, _)| self.ifaces.counts(name))
            .map(|(name, data)| ((*name).clone(), data.total_received(), data.total_transmitted()))
            .collect();

        let (rx_total, tx_total) = counted
            .iter()
            .fold((0u64, 0u64), |(rx, tx), (_, r, t)| (rx.saturating_add(*r), tx.saturating_add(*t)));

        let boot_id = epoch(&self.raw_boot_id, counted.iter().map(|(name, ..)| name.as_str()));
        let (rx, tx) = self.net_rate(&counted, Instant::now());
        let (tcp, udp) = conn_counts();
        let procs = self.sys.processes().len() as u32;

        let load = [
            (cpu / 100.0).clamp(0.0, 100.0),
            (cpu / 100.0).clamp(0.0, 100.0),
            (cpu / 100.0).clamp(0.0, 100.0),
        ];

        Metrics {
            boot_id,
            iface: self.ifaces.spec.clone(),
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

    fn net_rate<S: AsRef<str>>(&mut self, counted: &[(S, u64, u64)], now: Instant) -> (u64, u64) {
        let rate = match self.prev_net_at {
            Some(t) => {
                let secs = now.saturating_duration_since(t).as_secs_f64();
                let (rx, tx) = counted
                    .iter()
                    .filter_map(|(name, rx, tx)| {
                        let (prx, ptx) = self.prev_net.get(name.as_ref())?;
                        Some((rx.saturating_sub(*prx), tx.saturating_sub(*ptx)))
                    })
                    .fold((0u64, 0u64), |(a, b), (r, t)| (a.saturating_add(r), b.saturating_add(t)));
                if secs <= 0.0 {
                    (0, 0)
                } else {
                    ((rx as f64 / secs) as u64, (tx as f64 / secs) as u64)
                }
            }
            None => (0, 0),
        };
        self.prev_net_at = Some(now);
        self.prev_net = counted.iter().map(|(n, r, t)| (n.as_ref().to_owned(), (*r, *t))).collect();
        rate
    }
}

fn detect_virt() -> String {
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
    if lower == "lo" || lower.starts_with("lo") || lower.contains('.') {
        return true;
    }
    let skip = [
        "loopback", "isatap", "teredo", "vethernet", "docker", "veth", "br-", "virbr", "tap",
        "tun", "wg", "tailscale", "cni", "flannel", "podman", "zt", "fwln", "fwpr", "fwbr",
        "ifb", "gretap", "erspan", "lxc", "cilium", "kube", "cali", "nerdctl", "bond", "vlan",
        "vmbr", "pppoe-",
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
            if iface.is_loopback() || iface.is_link_local() || !iface.is_oper_up() || is_virtual_iface(&iface.name) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn ifaces_parsing() {
        let ifaces = Ifaces::parse("eth0, eth1, -eth2").unwrap();
        assert_eq!(ifaces.spec, "eth0,eth1,-eth2");
        assert_eq!(ifaces.only, vec!["eth0", "eth1"]);
        assert_eq!(ifaces.skip, vec!["eth2"]);

        assert!(ifaces.counts("eth0"));
        assert!(ifaces.counts("eth1"));
        assert!(!ifaces.counts("eth2"));
        assert!(!ifaces.counts("eth3"));

        let excluded_only = Ifaces::parse("-tailscale, -docker0").unwrap();
        assert_eq!(excluded_only.only.len(), 0);
        assert!(!excluded_only.counts("tailscale"));
        assert!(!excluded_only.counts("docker0"));
        assert!(excluded_only.counts("eth0"));

        assert!(Ifaces::parse("").unwrap().only.is_empty());
        assert!(Ifaces::parse("   ").unwrap().only.is_empty());
        assert!(Ifaces::parse("eth0, eth\n1").is_err());
        assert!(Ifaces::parse("--eth0").is_err());
        assert!(Ifaces::parse("eth0, -").is_err());
    }

    #[test]
    fn epoch_hash_changes_with_interface_set() {
        let boot = "boot-123456";
        let e1 = epoch(boot, ["eth0", "eth1"].into_iter());
        let e2 = epoch(boot, ["eth1", "eth0"].into_iter());
        assert_eq!(e1, e2, "order does not affect digest");

        let e3 = epoch(boot, ["eth0"].into_iter());
        assert_ne!(e1, e3, "different interface set produces different epoch");
    }

    #[test]
    fn public_ip_ranges() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        for s in [
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "100.64.0.1",
            "100.127.255.1",
            "127.0.0.1",
            "169.254.1.1",
            "0.0.0.1",
            "192.0.0.4",
            "198.18.0.1",
            "198.19.255.1",
            "224.0.0.1",
            "fd42::1",
            "fc00::1",
            "fe80::1",
            "::1",
        ] {
            assert!(!is_public(ip(s)), "{s}");
        }
        for s in [
            "1.1.1.1",
            "100.128.0.1",
            "198.20.0.1",
            "192.0.1.1",
            "223.5.5.5",
            "2401:b60:1c::5",
            "3fff::1",
        ] {
            assert!(is_public(ip(s)), "{s}");
        }
    }

    #[test]
    fn net_rate_calculation() {
        let mut collector = Collector::new(Ifaces::default());
        let t0 = Instant::now();
        let counted1 = [("eth0", 1000u64, 2000u64)];
        let (rx, tx) = collector.net_rate(&counted1, t0);
        assert_eq!((rx, tx), (0, 0), "first sample returns 0 rate");

        let t1 = t0 + Duration::from_secs(1);
        let counted2 = [("eth0", 2000u64, 4000u64)];
        let (rx, tx) = collector.net_rate(&counted2, t1);
        assert_eq!((rx, tx), (1000, 2000));

        // When a new interface appears, its baseline is recorded without causing a rate spike
        let t2 = t1 + Duration::from_secs(1);
        let counted3 = [("eth0", 3000u64, 6000u64), ("eth1", 100_000u64, 200_000u64)];
        let (rx, tx) = collector.net_rate(&counted3, t2);
        assert_eq!((rx, tx), (1000, 2000), "new interface does not cause rate burst");
    }
}
