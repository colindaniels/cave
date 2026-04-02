use anyhow::Result;
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm};

use crate::config::{scan_for_ip, Config};
use crate::ssh::{self, SshConnection};
use crate::ui;
use crate::vm;

pub async fn run(hostname: &str, force: bool) -> Result<()> {
    let mut config = Config::load()?;

    // Check if node exists
    let node = match config.get_node(hostname) {
        Some(n) => n.clone(),
        None => {
            ui::print_error(&format!("Node '{}' not found", hostname));
            return Ok(());
        }
    };

    // Confirm removal (skip if force flag)
    if !force {
        let theme = ColorfulTheme::default();
        let confirm = Confirm::with_theme(&theme)
            .with_prompt(format!("Remove node '{}' and wipe VM data?", hostname))
            .default(false)
            .interact()?;

        if !confirm {
            println!("{}", style("Cancelled").dim());
            return Ok(());
        }
    }

    // Try to connect and clean up VM data on the node
    let vm_name = format!("{}-vm", hostname);
    if let Some(ip) = scan_for_ip(&node.mac) {
        if let Ok(ssh) = SshConnection::connect(&ip) {
            // Stop VM if running
            if vm::is_vm_running(&ssh, &vm_name).unwrap_or(false) {
                let _ = vm::stop_vm(&ssh, &vm_name);
                ui::print_success("VM stopped");
            }

            // Delete VM disk, config, and seed ISO on the node
            let _ = vm::delete_vm(&ssh, &vm_name);
            ui::print_success("VM data wiped from node");
        } else {
            ui::print_warning("Could not connect to node - VM data may remain on disk");
        }
    } else {
        ui::print_warning("Node not found on network - VM data may remain on disk");
    }

    // Remove local VM config file
    let vm_config = Config::vms_dir().join(format!("{}.conf", hostname));
    if vm_config.exists() {
        let _ = std::fs::remove_file(&vm_config);
    }

    // Remove from config
    config.remove_node(hostname)?;
    config.save()?;

    // Remove from SSH config
    ssh::remove_ssh_config(hostname)?;

    // Also remove the VM's SSH config entry
    ssh::remove_ssh_config(&vm_name)?;

    ui::print_success(&format!("Node '{}' removed", hostname));

    Ok(())
}
