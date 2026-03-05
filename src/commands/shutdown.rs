use anyhow::Result;
use console::style;

use crate::config::{scan_for_ip, Config};
use crate::ssh::SshConnection;
use crate::ui;

pub async fn run(hostname: &str) -> Result<()> {
    let config = Config::load()?;

    let node = config
        .get_node(hostname)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", hostname))?;

    // Find node IP
    let ip = scan_for_ip(&node.mac).ok_or_else(|| {
        anyhow::anyhow!("Node '{}' not found on network. Is it powered on?", hostname)
    })?;

    // Connect and send poweroff
    let ssh = SshConnection::connect(&ip)?;

    // Use nohup to ensure poweroff completes even if SSH disconnects
    let _ = ssh.execute("nohup poweroff &");

    ui::print_success(&format!("Shutdown signal sent to {}", hostname));
    println!(
        "  {} {}",
        style("IP:").dim(),
        style(&ip).cyan()
    );
    println!(
        "  {} {}",
        style("Tip:").dim(),
        format!("Use '{}' to wake it back up", style("cave wake").cyan())
    );

    Ok(())
}
