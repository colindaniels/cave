use anyhow::Result;
use console::style;

use crate::config::Config;
use crate::ui;

pub async fn run(hostname: &str, mac: &str) -> Result<()> {
    // Validate MAC address format
    let mac = normalize_mac(mac)?;

    // Load config and add node
    let mut config = Config::load()?;
    config.add_node(hostname, &mac)?;
    config.save()?;

    ui::print_success(&format!("Node '{}' registered", hostname));
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
    println!(
        "  {} IP will be discovered automatically when the node is online",
        style("Note:").dim()
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
