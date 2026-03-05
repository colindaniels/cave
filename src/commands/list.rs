use anyhow::Result;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::config::Config;
use crate::ssh::SshConnection;
use crate::status::{get_node_info, DiskInfo, NodeStatus};
use crate::ui;
use crate::vm;

pub async fn run() -> Result<()> {
    let config = Config::load()?;

    if config.nodes.is_empty() {
        ui::print_warning("No nodes registered");
        println!(
            "  Run {} to add a node",
            style("cave init <hostname> <ip> <mac>").cyan()
        );
        return Ok(());
    }

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    spinner.set_message("Checking nodes...");
    spinner.enable_steady_tick(Duration::from_millis(80));

    let mut results = Vec::new();
    for node in &config.nodes {
        let info = get_node_info(node);
        let vm_info = if info.status == NodeStatus::Active {
            get_vm_info(&node.ip)
        } else {
            None
        };
        results.push((node.clone(), info, vm_info));
    }

    spinner.finish_and_clear();

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

    for (node, info, vm_info) in &results {
        // Status with color
        let status_display = match info.status {
            NodeStatus::Active => style("active").green().bold().to_string(),
            NodeStatus::Standby => style("standby").yellow().to_string(),
            NodeStatus::Offline => style("offline").red().dim().to_string(),
        };

        // Compact specs with disk info
        let specs = if info.status != NodeStatus::Offline {
            let disk_info = format_disk_summary(&info.specs.disks);
            format!(
                "{} {} {} {} {}",
                style(truncate(&info.specs.cpu, 18)).dim(),
                style("·").dim(),
                style(format!("{}c/{}", info.specs.cores.replace(" cores", ""), &info.specs.ram)).dim(),
                style("·").dim(),
                style(disk_info).dim()
            )
        } else {
            style("─").dim().to_string()
        };

        println!(
            "  {:<16} {:<16} {:<18} {}",
            style(&node.hostname).bold(),
            &node.ip,
            status_display,
            specs
        );

        // Show VM info if active
        if let Some(vm) = vm_info {
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
    let active = results.iter().filter(|(_, i, _)| i.status == NodeStatus::Active).count();
    let standby = results.iter().filter(|(_, i, _)| i.status == NodeStatus::Standby).count();
    let offline = results.iter().filter(|(_, i, _)| i.status == NodeStatus::Offline).count();

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

struct VmInfo {
    name: String,
    ip: String,
    memory: String,
    cpus: String,
}

fn get_vm_info(host_ip: &str) -> Option<VmInfo> {
    let ssh = SshConnection::connect(host_ip).ok()?;

    let output = ssh
        .execute(&format!(
            r#"for pid in {}/*.pid; do
            [ -f "$pid" ] && kill -0 $(cat "$pid") 2>/dev/null && {{
                vm=$(basename "$pid" .pid)
                qemu_args=$(cat /proc/$(cat "$pid")/cmdline 2>/dev/null | tr '\0' ' ')
                # Extract MAC from QEMU args and scan network to find its IP
                mac=$(echo "$qemu_args" | grep -o 'mac=[^, ]*' | cut -d= -f2)
                # Install arp-scan if needed, then scan for the MAC
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
        Some(VmInfo {
            name: parts[0].to_string(),
            ip: if parts[1].is_empty() {
                String::new()
            } else {
                parts[1].to_string()
            },
            memory: format!("{}M", parts[2]),
            cpus: parts[3].to_string(),
        })
    } else {
        None
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

fn format_disk_summary(disks: &[DiskInfo]) -> String {
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
