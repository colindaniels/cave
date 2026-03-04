use anyhow::{Context, Result};

use crate::config::Config;
use crate::ssh::SshConnection;
use crate::vm;

pub async fn run(hostname: &str) -> Result<()> {
    let config = Config::load()?;

    // Find the node
    let node = config.get_node(hostname)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found. Use 'cave list' to see registered nodes.", hostname))?;

    // Connect via SSH to the Alpine host
    println!("Connecting to {} (Alpine host)...", node.ip);
    let ssh = SshConnection::connect(&node.ip)
        .context("Failed to connect to node via SSH")?;

    // Check if VM is running
    if !vm::is_vm_running(&ssh, hostname)? {
        println!("No VM '{}' is running on this node.", hostname);
        return Ok(());
    }

    // Stop and delete the VM
    println!("Stopping VM '{}'...", hostname);
    vm::delete_vm(&ssh, hostname)?;

    println!("\n=== Destroy Complete ===");
    println!("VM '{}' has been stopped and removed.", hostname);
    println!("Node is back in standby mode (Alpine host still running).");
    println!("Use 'cave deploy {}' to deploy a new VM.", hostname);

    Ok(())
}
