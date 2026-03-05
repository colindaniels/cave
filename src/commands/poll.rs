use anyhow::Result;
use std::thread;

use crate::config::{save_ip_cache, save_node_cache, scan_network, CachedDisk, CachedNode, CachedVm, Config};
use crate::ssh::{self, SshConnection};
use crate::status::{get_node_info, NodeStatus};
use crate::vm;

/// Background poll: scan network, update IP cache, SSH config, and full node cache
pub async fn run() -> Result<()> {
    let config = Config::load()?;

    if config.nodes.is_empty() {
        return Ok(());
    }

    // Scan network
    let mac_to_ip = scan_network();

    // Update SSH config and gather full node info in parallel
    let handles: Vec<_> = config
        .nodes
        .iter()
        .map(|node| {
            let node = node.clone();
            let ip = mac_to_ip.get(&node.mac.to_lowercase()).cloned();
            thread::spawn(move || {
                // Get node info first to check if it's actually online
                let info = get_node_info(&node, ip.as_deref());

                // Only use IP if node actually responded (not offline)
                let verified_ip = if info.status != NodeStatus::Offline {
                    // Node responded - IP is valid, update SSH config
                    if let Some(ref ip) = ip {
                        let _ = ssh::update_ssh_config(&node.hostname, ip);
                    }
                    ip
                } else {
                    // Node didn't respond - don't trust cached IP
                    None
                };

                // Get VM info if active
                let vm_info = if info.status == NodeStatus::Active {
                    verified_ip.as_ref().and_then(|ip| get_vm_info(ip))
                } else {
                    None
                };

                CachedNode {
                    hostname: node.hostname.clone(),
                    mac: node.mac.clone(),
                    ip: verified_ip,
                    status: info.status.to_string(),
                    cpu: info.specs.cpu,
                    cores: info.specs.cores,
                    ram: info.specs.ram,
                    disks: info.specs.disks.iter().map(|d| CachedDisk {
                        size_bytes: d.size_bytes,
                        disk_type: d.disk_type.clone(),
                    }).collect(),
                    vm: vm_info,
                }
            })
        })
        .collect();

    let cached_nodes: Vec<_> = handles
        .into_iter()
        .filter_map(|h| h.join().ok())
        .collect();

    // Build verified IP cache from nodes that actually responded
    let verified_ips: std::collections::HashMap<String, String> = cached_nodes
        .iter()
        .filter_map(|n| {
            n.ip.as_ref().map(|ip| (n.mac.to_lowercase(), ip.clone()))
        })
        .collect();

    // Save caches
    let _ = save_ip_cache(&verified_ips);
    let _ = save_node_cache(&cached_nodes);

    Ok(())
}

fn get_vm_info(host_ip: &str) -> Option<CachedVm> {
    let ssh = SshConnection::connect(host_ip).ok()?;

    let output = ssh
        .execute(&format!(
            r#"for pid in {}/*.pid; do
            [ -f "$pid" ] && kill -0 $(cat "$pid") 2>/dev/null && {{
                vm=$(basename "$pid" .pid)
                qemu_args=$(cat /proc/$(cat "$pid")/cmdline 2>/dev/null | tr '\0' ' ')
                mac=$(echo "$qemu_args" | grep -o 'mac=[^, ]*' | cut -d= -f2)
                which arp-scan >/dev/null 2>&1 || apk add --no-cache arp-scan >/dev/null 2>&1
                ip=$(arp-scan -I br0 -l 2>/dev/null | grep -i "$mac" | awk '{{print $1}}')
                mem=$(echo "$qemu_args" | sed -n 's/.*-m \([0-9]*\).*/\1/p')
                cpus=$(echo "$qemu_args" | sed -n 's/.*-smp \([0-9]*\).*/\1/p')
                echo "$vm|$ip|$mem|$cpus"
                exit 0
            }}
        done"#,
            vm::VM_RUN_PATH
        ))
        .ok()?;

    let parts: Vec<&str> = output.trim().split('|').collect();
    if parts.len() >= 4 && !parts[0].is_empty() {
        Some(CachedVm {
            name: parts[0].to_string(),
            ip: parts[1].to_string(),
            memory: format!("{}M", parts[2]),
            cpus: parts[3].to_string(),
        })
    } else {
        None
    }
}
