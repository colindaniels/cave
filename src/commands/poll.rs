use anyhow::Result;
use std::thread;

use std::collections::HashSet;
use std::time::Duration;

use crate::config::{save_discovered_cache, save_ip_cache, save_node_cache, scan_network, CachedDisk, CachedNode, CachedVm, Config};
use crate::ssh::{self, SshConnection};
use crate::status::{get_node_info, NodeStatus};
use crate::vm;

/// Background poll: scan network, update IP cache, SSH config, and full node cache
pub async fn run() -> Result<()> {
    // Ensure SSH include is set up (for existing installs)
    let _ = ssh::setup_ssh_include();

    let config = Config::load()?;

    // Scan network
    let mac_to_ip = scan_network();

    if config.nodes.is_empty() {
        // Still discover unknown nodes even with no registered nodes
        discover_unknown_nodes(&config, &mac_to_ip, &[]);
        return Ok(());
    }

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

                // Get RAM usage for any online node, VM info only if active
                let (ram_total_mb, ram_used_mb) = if info.status != NodeStatus::Offline {
                    verified_ip.as_ref()
                        .map(|ip| get_host_ram_usage(ip))
                        .unwrap_or((None, None))
                } else {
                    (None, None)
                };

                let vm_info = if info.status == NodeStatus::Active {
                    verified_ip.as_ref().and_then(|ip| get_vm_info(ip))
                } else {
                    None
                };

                // Update SSH config for VM if it has an IP
                if let Some(ref vm) = vm_info {
                    if !vm.ip.is_empty() {
                        let _ = ssh::update_ssh_config(&vm.name, &vm.ip);
                    }
                }

                CachedNode {
                    hostname: node.hostname.clone(),
                    mac: node.mac.clone(),
                    ip: verified_ip,
                    status: info.status.to_string(),
                    cpu: info.specs.cpu,
                    cores: info.specs.cores,
                    ram: info.specs.ram,
                    ram_total_mb,
                    ram_used_mb,
                    disks: info.specs.disks.iter().map(|d| CachedDisk {
                        name: d.name.clone(),
                        size_bytes: d.size_bytes,
                        disk_type: d.disk_type.clone(),
                        model: d.model.clone(),
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

    // Discover unknown PXE-booted nodes
    discover_unknown_nodes(&config, &mac_to_ip, &cached_nodes);

    Ok(())
}

/// Check unknown MACs on the network - if they respond to our SSH key, they PXE-booted from us
fn discover_unknown_nodes(config: &Config, mac_to_ip: &std::collections::HashMap<String, String>, cached_nodes: &[CachedNode]) {
    // Collect all known MACs: registered nodes + their VMs
    let mut known_macs: HashSet<String> = config.nodes.iter()
        .map(|n| n.mac.to_lowercase())
        .collect();

    // Also exclude VM MACs (generated from VM name)
    for node in cached_nodes {
        if let Some(ref vm) = node.vm {
            let vm_mac = vm::generate_mac_for_lookup(&vm.name).to_lowercase();
            known_macs.insert(vm_mac);
        }
    }

    let unknown: Vec<(String, String)> = mac_to_ip.iter()
        .filter(|(mac, _)| !known_macs.contains(mac.as_str()))
        .map(|(mac, ip)| (mac.clone(), ip.clone()))
        .collect();

    if unknown.is_empty() {
        let _ = save_discovered_cache(&[]);
        return;
    }

    // Probe unknown MACs with short timeout and gather full info in parallel
    let handles: Vec<_> = unknown.into_iter()
        .take(20) // Limit to avoid flooding
        .map(|(mac, ip)| {
            thread::spawn(move || {
                // Quick SSH probe - if it fails, not a PXE node
                let ssh = SshConnection::connect_timeout(&ip, Duration::from_secs(2)).ok()?;
                drop(ssh);

                // It responded - gather full info like a registered node
                let dummy_node = crate::config::Node {
                    hostname: String::new(),
                    mac: mac.clone(),
                };
                let info = get_node_info(&dummy_node, Some(&ip));

                let (ram_total_mb, ram_used_mb) = get_host_ram_usage(&ip);

                let vm_info = if info.status == NodeStatus::Active {
                    get_vm_info(&ip)
                } else {
                    None
                };

                Some(CachedNode {
                    hostname: String::new(), // Empty = discovered
                    mac,
                    ip: Some(ip),
                    status: info.status.to_string(),
                    cpu: info.specs.cpu,
                    cores: info.specs.cores,
                    ram: info.specs.ram,
                    ram_total_mb,
                    ram_used_mb,
                    disks: info.specs.disks.iter().map(|d| CachedDisk {
                        name: d.name.clone(),
                        size_bytes: d.size_bytes,
                        disk_type: d.disk_type.clone(),
                        model: d.model.clone(),
                    }).collect(),
                    vm: vm_info,
                })
            })
        })
        .collect();

    let discovered: Vec<CachedNode> = handles.into_iter()
        .filter_map(|h| h.join().ok().flatten())
        .collect();

    let _ = save_discovered_cache(&discovered);
}

/// Get host RAM usage (total, used) in MB
fn get_host_ram_usage(host_ip: &str) -> (Option<u64>, Option<u64>) {
    let Ok(ssh) = SshConnection::connect(host_ip) else {
        return (None, None);
    };

    // Get total and used RAM from host
    // free -m output: "Mem: total used free shared buff/cache available"
    let Ok(output) = ssh.execute("free -m | awk '/Mem:/ {print $2\"|\"$3}'") else {
        return (None, None);
    };

    let parts: Vec<&str> = output.trim().split('|').collect();
    if parts.len() >= 2 {
        let total = parts[0].parse::<u64>().ok();
        let used = parts[1].parse::<u64>().ok();
        (total, used)
    } else {
        (None, None)
    }
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
                ip=$(arp-scan -I br0 -l 2>/dev/null | grep -i "$mac" | awk '{{print $1}}' | head -1)
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
        let vm_ip = parts[1].to_string();
        let allocated_mem = parts[2].to_string();

        // Get VM's actual memory usage if we have its IP
        let memory_used_mb = if !vm_ip.is_empty() {
            get_vm_memory_usage(&vm_ip)
        } else {
            None
        };

        Some(CachedVm {
            name: parts[0].to_string(),
            ip: vm_ip,
            memory: format!("{}M", allocated_mem),
            memory_used_mb,
            cpus: parts[3].to_string(),
        })
    } else {
        None
    }
}

/// Get VM's actual memory usage by SSHing into the VM
fn get_vm_memory_usage(vm_ip: &str) -> Option<u64> {
    // Try to SSH into the VM (may fail if VM not ready or no SSH)
    let ssh = SshConnection::connect(vm_ip).ok()?;
    let output = ssh.execute("free -m | awk '/Mem:/ {print $3}'").ok()?;
    output.trim().parse::<u64>().ok()
}
