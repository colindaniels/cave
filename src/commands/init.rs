use anyhow::Result;
use console::style;
use std::time::Duration;

use crate::config::{scan_for_ip, Config};
use crate::ssh::SshConnection;
use crate::ui;

pub async fn run(hostname: &str, mac: &str) -> Result<()> {
    // Validate MAC address format
    let mac = normalize_mac(mac)?;

    // Load config and add node
    let mut config = Config::load()?;
    config.add_node(hostname, &mac)?;
    config.save()?;

    ui::print_success(&format!("Node '{}' registered", hostname));

    // Try to connect and wipe all disks
    if let Some(ip) = scan_for_ip(&mac) {
        println!();
        println!(
            "  {} Wiping all disks on {}...",
            style("→").cyan().bold(),
            style(&ip).dim()
        );

        if let Ok(ssh) = SshConnection::connect(&ip) {
            // Find all non-removable block devices (HDDs, SSDs, NVMes)
            let (disks_output, _) = ssh.execute_with_status(
                r#"for d in /sys/block/*; do
                    name=$(basename "$d")
                    case "$name" in sd*|nvme*|vd*) ;; *) continue ;; esac
                    removable=$(cat "$d/removable" 2>/dev/null || echo 1)
                    [ "$removable" = "0" ] && echo "$name"
                done"#
            )?;

            let disks: Vec<&str> = disks_output.trim().lines().filter(|l| !l.is_empty()).collect();

            if disks.is_empty() {
                ui::print_warning("No disks found to wipe");
            } else {
                // Set longer timeout for large disk operations
                ssh.set_timeout(Duration::from_secs(300));

                for disk in &disks {
                    println!("  Wiping /dev/{}...", disk);
                    // Wipe partition table and first 100MB
                    let _ = ssh.execute(&format!(
                        "dd if=/dev/zero of=/dev/{} bs=1M count=100 2>/dev/null", disk
                    ));
                    // Also wipe the end of disk (GPT backup header)
                    let _ = ssh.execute(&format!(
                        "dd if=/dev/zero of=/dev/{} bs=1M seek=$(($(cat /sys/block/{}/size) * 512 / 1048576 - 10)) count=10 2>/dev/null",
                        disk, disk
                    ));
                }

                ssh.set_timeout(Duration::from_secs(10));
                ui::print_success(&format!("{} disk(s) wiped: {}", disks.len(), disks.join(", ")));
            }
        } else {
            ui::print_warning("Could not connect to node - disks not wiped");
        }
    } else {
        ui::print_warning("Node not found on network - disks will be wiped when node comes online");
    }

    println!();
    ui::print_box("Node Details", &[
        ("Hostname", hostname),
        ("MAC", &mac),
    ]);
    println!();
    println!(
        "  {} {}",
        style("Next:").dim(),
        style("cave deploy").cyan()
    );

    Ok(())
}

fn normalize_mac(mac: &str) -> Result<String> {
    let cleaned: String = mac
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_lowercase();

    if cleaned.len() != 12 {
        anyhow::bail!(
            "Invalid MAC address: {}. Expected 12 hex digits.",
            style(mac).red()
        );
    }

    let formatted = cleaned
        .chars()
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(":");

    Ok(formatted)
}
