use anyhow::Result;
use console::style;

use crate::config::{load_node_cache, CachedDisk, Config};
use crate::ui;

pub async fn run() -> Result<()> {
    let config = Config::load()?;

    if config.nodes.is_empty() {
        ui::print_warning("No nodes registered");
        println!(
            "  Run {} to add a node",
            style("cave init <hostname> <mac>").cyan()
        );
        return Ok(());
    }

    // Load from cache - instant!
    let cached = load_node_cache();

    // If cache is empty, tell user to wait for poller
    if cached.is_empty() {
        ui::print_warning("Node cache is empty - waiting for background poll");
        println!(
            "  The background poller updates every 10 seconds. Try again shortly."
        );
        return Ok(());
    }

    // Print header
    println!();
    println!(
        "  {}",
        style(format!(
            "{:<16} {:<16} {:<10} {}",
            "NAME", "IP", "STATUS", "SPECS"
        ))
        .dim()
    );
    println!("  {}", style("─".repeat(70)).dim());

    for node in &cached {
        // Status with color
        let status_display = match node.status.as_str() {
            "active" => style("active").green().bold().to_string(),
            "standby" => style("standby").yellow().to_string(),
            _ => style("offline").red().dim().to_string(),
        };

        // Compact specs with disk info
        let specs = if node.status != "offline" {
            let disk_info = format_disk_summary(&node.disks);
            format!(
                "{} {} {} {} {}",
                style(truncate(&node.cpu, 18)).dim(),
                style("·").dim(),
                style(format!("{}c/{}", node.cores.replace(" cores", ""), &node.ram)).dim(),
                style("·").dim(),
                style(disk_info).dim()
            )
        } else {
            style("─").dim().to_string()
        };

        // Display IP only if online, otherwise show "─"
        let ip_display = if node.status == "offline" {
            "─"
        } else {
            node.ip.as_ref().map(|s| s.as_str()).unwrap_or("─")
        };

        println!(
            "  {:<16} {:<16} {:<18} {}",
            style(&node.hostname).bold(),
            ip_display,
            status_display,
            specs
        );

        // Show VM info if present
        if let Some(vm) = &node.vm {
            let vm_status = style("running").green().to_string();
            let vm_specs = format!("{}, {} CPU", vm.memory, vm.cpus);

            println!(
                "  {:<16} {:<16} {:<18} {}",
                style(format!("└─ {}", vm.name)).cyan(),
                if vm.ip.is_empty() {
                    style("booting...").dim().to_string()
                } else {
                    vm.ip.clone()
                },
                vm_status,
                style(vm_specs).dim()
            );
        }
    }

    println!();

    // Summary
    let active = cached.iter().filter(|n| n.status == "active").count();
    let standby = cached.iter().filter(|n| n.status == "standby").count();
    let offline = cached.iter().filter(|n| n.status == "offline").count();

    print!("  ");
    if active > 0 {
        print!("{} ", style(format!("{} active", active)).green());
    }
    if standby > 0 {
        print!("{} ", style(format!("{} standby", standby)).yellow());
    }
    if offline > 0 {
        print!("{} ", style(format!("{} offline", offline)).red().dim());
    }
    println!();

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

fn format_disk_summary(disks: &[CachedDisk]) -> String {
    if disks.is_empty() {
        return "no disks".to_string();
    }

    disks
        .iter()
        .map(|d| {
            let size = format_bytes(d.size_bytes);
            let dtype = if d.disk_type == "SSD" { "SSD" } else { "HDD" };
            format!("{} {}", size, dtype)
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn format_bytes(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const TB: u64 = 1_000_000_000_000;

    if bytes >= TB {
        format!("{:.1}T", bytes as f64 / TB as f64)
    } else {
        format!("{}G", bytes / GB)
    }
}
