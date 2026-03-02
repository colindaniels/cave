use anyhow::Result;
use tabled::{Table, Tabled};

use crate::config::Config;
use crate::status::{get_node_info, NodeStatus};

#[derive(Tabled)]
struct NodeRow {
    #[tabled(rename = "HOSTNAME")]
    hostname: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "MAC")]
    mac: String,
    #[tabled(rename = "STATUS")]
    status: String,
    #[tabled(rename = "CPU")]
    cpu: String,
    #[tabled(rename = "CORES")]
    cores: String,
    #[tabled(rename = "RAM")]
    ram: String,
    #[tabled(rename = "GPU")]
    gpu: String,
}

pub async fn run() -> Result<()> {
    let config = Config::load()?;

    if config.nodes.is_empty() {
        println!("No nodes registered. Use 'cave init <hostname> <ip> <mac>' to add a node.");
        return Ok(());
    }

    println!("Checking node status...\n");

    let mut rows: Vec<NodeRow> = Vec::new();

    for node in &config.nodes {
        let info = get_node_info(node);

        let status_str = match info.status {
            NodeStatus::Offline => "offline".to_string(),
            NodeStatus::Standby => "standby".to_string(),
            NodeStatus::Active => "active".to_string(),
        };

        rows.push(NodeRow {
            hostname: info.hostname,
            ip: info.ip,
            mac: info.mac,
            status: status_str,
            cpu: truncate(&info.specs.cpu, 30),
            cores: info.specs.cores,
            ram: info.specs.ram,
            gpu: truncate(&info.specs.gpu, 25),
        });
    }

    let table = Table::new(rows).to_string();
    println!("{}", table);

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
