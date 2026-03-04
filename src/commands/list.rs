use anyhow::Result;
use tabled::{Table, Tabled};

use crate::config::Config;
use crate::ssh::SshConnection;
use crate::status::{get_node_info, NodeStatus};
use crate::vm;

#[derive(Tabled)]
struct NodeRow {
    #[tabled(rename = "HOSTNAME")]
    hostname: String,
    #[tabled(rename = "IP (HOST)")]
    ip: String,
    #[tabled(rename = "IP (VM)")]
    vm_ip: String,
    #[tabled(rename = "MAC")]
    mac: String,
    #[tabled(rename = "STATUS")]
    status: String,
    #[tabled(rename = "CPU")]
    cpu: String,
    #[tabled(rename = "CORES")]
    cores: String,
    #[tabled(rename = "RAM")]
    ram: String,
}

pub async fn run() -> Result<()> {
    let config = Config::load()?;

    if config.nodes.is_empty() {
        println!("No nodes registered. Use 'cave init <hostname> <ip> <mac>' to add a node.");
        return Ok(());
    }

    println!("Checking node status...\n");

    let mut rows: Vec<NodeRow> = Vec::new();

    for node in &config.nodes {
        let info = get_node_info(node);

        let (status_str, vm_ip) = match info.status {
            NodeStatus::Offline => ("offline".to_string(), "-".to_string()),
            NodeStatus::Standby => ("standby".to_string(), "-".to_string()),
            NodeStatus::Active => {
                // Try to get VM IP
                let vm_ip = get_vm_ip(&node.ip, &node.hostname).unwrap_or_else(|| "booting...".to_string());
                ("active".to_string(), vm_ip)
            }
        };

        rows.push(NodeRow {
            hostname: info.hostname,
            ip: info.ip,
            vm_ip,
            mac: info.mac,
            status: status_str,
            cpu: truncate(&info.specs.cpu, 25),
            cores: info.specs.cores,
            ram: info.specs.ram,
        });
    }

    let table = Table::new(rows).to_string();
    println!("{}", table);

    Ok(())
}

fn get_vm_ip(host_ip: &str, vm_name: &str) -> Option<String> {
    let ssh = SshConnection::connect(host_ip).ok()?;

    // First, try to read IP from serial console log (cloud-init prints it)
    // Format: ci-info: |  ens3  | True |       192.168.1.106       |
    let log_path = format!("{}/{}.log", vm::VM_RUN_PATH, vm_name);
    let output = ssh.execute(&format!(
        "grep 'ci-info:.*ens.*True' {} 2>/dev/null | sed 's/.*True[^0-9]*\\([0-9.]\\+\\).*/\\1/' | head -1",
        log_path
    )).ok()?;

    let ip = output.trim();
    if !ip.is_empty() {
        return Some(ip.to_string());
    }

    // Fallback: try ARP table lookup after pinging subnet
    let vm_mac = vm::generate_mac_for_lookup(vm_name);

    // Do a quick ping sweep to populate ARP table
    let _ = ssh.execute("for i in $(seq 1 254); do ping -c 1 -W 1 192.168.1.$i >/dev/null 2>&1 & done; sleep 2");

    let output = ssh.execute(&format!(
        "ip neigh show | grep -i '{}' | awk '{{print $1}}' | head -1",
        vm_mac
    )).ok()?;

    let ip = output.trim();
    if ip.is_empty() {
        None
    } else {
        Some(ip.to_string())
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
