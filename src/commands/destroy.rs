use anyhow::{Context, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::config::{scan_for_ip, Config};
use crate::ssh::SshConnection;
use crate::ui;
use crate::vm;

pub async fn run(hostname: &str, vm_name: &str) -> Result<()> {
    let config = Config::load()?;
    let theme = ColorfulTheme::default();

    // Find the node
    let node = config
        .get_node(hostname)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", hostname))?;

    // Scan network for node IP
    let node_ip = scan_for_ip(&node.mac).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not find node '{}' on network. Is it powered on?",
            node.hostname
        )
    })?;

    // Connect via SSH
    let spinner = create_spinner(&format!("Connecting to {}...", node_ip));
    let ssh = SshConnection::connect(&node_ip).context("Failed to connect via SSH")?;
    spinner.finish_and_clear();

    // Check if VM is running
    if !vm::is_vm_running(&ssh, vm_name)? {
        ui::print_warning(&format!("No VM '{}' running on this node", vm_name));
        return Ok(());
    }

    // Confirm destruction
    println!();
    println!(
        "  {} {} on {}",
        style("VM:").dim(),
        style(vm_name).cyan().bold(),
        style(&node.hostname).cyan()
    );
    println!();

    let confirm = Confirm::with_theme(&theme)
        .with_prompt(format!("Destroy VM '{}'?", vm_name))
        .default(false)
        .interact()?;

    if !confirm {
        println!("{}", style("Cancelled").dim());
        return Ok(());
    }

    // Stop and delete the VM
    let spinner = create_spinner("Stopping VM...");
    vm::delete_vm(&ssh, vm_name)?;
    spinner.finish_and_clear();

    ui::print_completion("VM Destroyed");
    println!();
    println!(
        "  {} {}",
        style("Node:").dim(),
        format!("{} is now in standby", style(&node.hostname).cyan())
    );
    println!(
        "  {} {}",
        style("Deploy:").dim(),
        style(format!("cave deploy {}", hostname)).cyan()
    );

    Ok(())
}

fn create_spinner(message: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    spinner.set_message(message.to_string());
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner
}
