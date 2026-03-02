use anyhow::{Context, Result};

use crate::config::Config;
use crate::ssh::SshConnection;
use crate::status::{get_node_status, NodeStatus};

pub async fn run(hostname: &str) -> Result<()> {
    let config = Config::load()?;

    // Find the node
    let node = config.get_node(hostname)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found. Use 'cave list' to see registered nodes.", hostname))?;

    // Check node status
    println!("Checking node status...");
    let status = get_node_status(node);

    match status {
        NodeStatus::Offline => {
            anyhow::bail!("Node '{}' is offline. Cannot destroy.", hostname);
        }
        NodeStatus::Standby => {
            println!("Node '{}' is already in standby mode.", hostname);
            return Ok(());
        }
        NodeStatus::Active => {
            println!("Node '{}' has an active deployment. Proceeding to destroy.", hostname);
        }
    }

    // Connect via SSH
    println!("Connecting to {}...", node.ip);
    let ssh = SshConnection::connect(&node.ip)
        .context("Failed to connect to node via SSH")?;

    // Find the target drive (prefer NVMe, fallback to sda)
    println!("Detecting target drive...");
    let (drive_output, _) = ssh.execute_with_status(
        "ls /dev/nvme0n1 2>/dev/null || ls /dev/sda 2>/dev/null"
    )?;

    let target_device = drive_output.trim().to_string();
    if target_device.is_empty() {
        anyhow::bail!("Could not detect target drive on node");
    }
    println!("Target drive: {}", target_device);

    // Wipe the partition table / first 100MB
    println!("Wiping drive partition table...");
    let wipe_command = format!(
        "dd if=/dev/zero of={} bs=4M count=100 conv=fsync 2>&1",
        target_device
    );

    let (output, status) = ssh.execute_with_status(&wipe_command)?;

    if status != 0 {
        println!("Warning: wipe command returned status {}", status);
        println!("{}", output);
    } else {
        println!("Drive wiped successfully");
    }

    // Sync
    let _ = ssh.execute("sync");

    // Reboot - this will cause PXE boot since drive has no OS
    println!("Rebooting node...");
    let _ = ssh.execute("nohup sh -c 'sleep 2 && reboot' &");

    println!("\nDestroy complete!");
    println!("Node '{}' is rebooting and will return to standby mode via PXE.", hostname);
    println!("Use 'cave list' to check status after reboot.");

    Ok(())
}
