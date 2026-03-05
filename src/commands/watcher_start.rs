use anyhow::Result;
use std::fs;

use crate::config::Config;
use crate::ssh::SshConnection;
use crate::vm;

/// Start a VM on a node (called by watcher after node reboots)
/// This does exactly what deploy does, but skips image transfer since it's already there
pub async fn run(hostname: &str) -> Result<()> {
    // Read VM config from server
    let config_path = Config::vms_dir().join(format!("{}.conf", hostname));
    if !config_path.exists() {
        return Ok(()); // No VM configured for this node
    }

    let config_content = fs::read_to_string(&config_path)?;

    // Parse config
    let mut node_ip = String::new();
    let mut vm_name = String::new();
    let mut memory_mb: u32 = 2048;
    let mut cpus: u32 = 2;
    let mut disk_path = String::new();
    let mut seed_iso = String::new();
    let mut disk_name = String::new();
    let mut mac = String::new();

    for line in config_content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "NODE_IP" => node_ip = value.to_string(),
                "VM_NAME" => vm_name = value.to_string(),
                "MEMORY_MB" => memory_mb = value.parse().unwrap_or(2048),
                "CPUS" => cpus = value.parse().unwrap_or(2),
                "DISK_PATH" => disk_path = value.to_string(),
                "SEED_ISO" => seed_iso = value.to_string(),
                "DISK_NAME" => disk_name = value.to_string(),
                "MAC" => mac = value.to_string(),
                _ => {}
            }
        }
    }

    if node_ip.is_empty() || vm_name.is_empty() || disk_path.is_empty() {
        return Ok(()); // Invalid config
    }

    // Connect to node
    let ssh = match SshConnection::connect(&node_ip) {
        Ok(s) => s,
        Err(_) => return Ok(()), // Node not reachable
    };

    // Check if VM is already running
    if vm::is_vm_running(&ssh, &vm_name)? {
        return Ok(()); // Already running
    }

    // Mount storage if disk name specified
    if !disk_name.is_empty() {
        vm::mount_storage(&ssh, &disk_name)?;
    }

    // Check if VM disk exists
    let (output, _) = ssh.execute_with_status(&format!("test -f '{}' && echo yes", disk_path))?;
    if output.trim() != "yes" {
        return Ok(()); // Disk doesn't exist
    }

    // Set up hypervisor (same as deploy)
    vm::setup_hypervisor(&ssh)?;

    // Start VM with the saved MAC address
    let seed_path = if seed_iso.is_empty() { None } else { Some(seed_iso.as_str()) };
    vm::start_vm_with_mac(&ssh, &vm_name, &disk_path, seed_path, memory_mb, cpus, None, &mac)?;

    println!("Started VM {} on {}", vm_name, node_ip);
    Ok(())
}
