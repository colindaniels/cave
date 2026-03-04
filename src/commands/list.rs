use anyhow::Result;
use tabled::{Table, Tabled, settings::Style};

use crate::config::Config;
use crate::ssh::SshConnection;
use crate::status::{get_node_info, NodeStatus};
use crate::vm;

#[derive(Tabled)]
struct ListRow {
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "STATUS")]
    status: String,
    #[tabled(rename = "SPECS")]
    specs: String,
}

struct VmInfo {
    name: String,
    ip: String,
    memory: String,
    cpus: String,
}

pub async fn run() -> Result<()> {
    let config = Config::load()?;

    if config.nodes.is_empty() {
        println!("No nodes registered. Use 'cave init <hostname> <ip> <mac>' to add a node.");
        return Ok(());
    }

    println!("Checking node status...\n");

    let mut rows: Vec<ListRow> = Vec::new();

    for node in &config.nodes {
        let info = get_node_info(node);

        // Compact hardware specs
        let hw_specs = format!(
            "{} · {} · {}",
            truncate(&info.specs.cpu, 20),
            info.specs.cores.replace(" cores", "c"),
            info.specs.ram
        );

        let status_str = match info.status {
            NodeStatus::Offline => "offline",
            NodeStatus::Standby => "standby",
            NodeStatus::Active => "active",
        };

        // Add node row
        rows.push(ListRow {
            name: node.hostname.clone(),
            ip: node.ip.clone(),
            status: status_str.to_string(),
            specs: hw_specs,
        });

        // If active, add VM row underneath
        if info.status == NodeStatus::Active {
            if let Some(vm_info) = get_vm_info(&node.ip) {
                let vm_specs = format!("{} · {} CPU", vm_info.memory, vm_info.cpus);
                rows.push(ListRow {
                    name: format!(" └ {}", vm_info.name),
                    ip: vm_info.ip,
                    status: "running".to_string(),
                    specs: vm_specs,
                });
            }
        }
    }

    let mut table = Table::new(rows);
    table.with(Style::rounded());
    println!("{}", table);

    Ok(())
}

fn get_vm_info(host_ip: &str) -> Option<VmInfo> {
    let ssh = SshConnection::connect(host_ip).ok()?;

    // Get VM name, IP, and specs from running QEMU process
    let output = ssh.execute(&format!(
        r#"for pid in {}/*.pid; do
            [ -f "$pid" ] && kill -0 $(cat "$pid") 2>/dev/null && {{
                vm=$(basename "$pid" .pid)
                # Get IP from log
                ip=$(grep 'ci-info:.*ens.*True' "{}/$vm.log" 2>/dev/null | sed 's/.*True[^0-9]*\([0-9.]\+\).*/\1/' | head -1)
                # Get memory and cpus from QEMU process args
                qemu_args=$(cat /proc/$(cat "$pid")/cmdline 2>/dev/null | tr '\0' ' ')
                mem=$(echo "$qemu_args" | sed -n 's/.*-m \([0-9]*\).*/\1/p')
                cpus=$(echo "$qemu_args" | sed -n 's/.*-smp \([0-9]*\).*/\1/p')
                echo "$vm|$ip|$mem|$cpus"
                exit 0
            }}
        done"#,
        vm::VM_RUN_PATH, vm::VM_RUN_PATH
    )).ok()?;

    let parts: Vec<&str> = output.trim().split('|').collect();
    if parts.len() >= 4 && !parts[0].is_empty() {
        Some(VmInfo {
            name: parts[0].to_string(),
            ip: if parts[1].is_empty() { "booting...".to_string() } else { parts[1].to_string() },
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
