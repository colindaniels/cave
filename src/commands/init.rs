use anyhow::Result;

use crate::config::Config;
use crate::ssh;

pub async fn run(hostname: &str, ip: &str, mac: &str) -> Result<()> {
    // Validate MAC address format
    let mac = normalize_mac(mac)?;

    // Validate IP address format
    validate_ip(ip)?;

    // Load config and add node
    let mut config = Config::load()?;
    config.add_node(hostname, ip, &mac)?;
    config.save()?;

    // Update SSH config
    ssh::update_ssh_config(hostname, ip)?;

    println!("Node '{}' registered successfully", hostname);
    println!("  IP: {}", ip);
    println!("  MAC: {}", mac);
    println!("\nYou can now SSH to the node with: ssh {}", hostname);

    Ok(())
}

fn normalize_mac(mac: &str) -> Result<String> {
    // Accept formats like: aa:bb:cc:dd:ee:ff, aa-bb-cc-dd-ee-ff, aabbccddeeff
    let cleaned: String = mac
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_lowercase();

    if cleaned.len() != 12 {
        anyhow::bail!("Invalid MAC address: {}. Expected 12 hex digits.", mac);
    }

    // Format as aa:bb:cc:dd:ee:ff
    let formatted = cleaned
        .chars()
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(":");

    Ok(formatted)
}

fn validate_ip(ip: &str) -> Result<()> {
    let parts: Vec<&str> = ip.split('.').collect();

    if parts.len() != 4 {
        anyhow::bail!("Invalid IP address: {}. Expected format: x.x.x.x", ip);
    }

    for part in parts {
        match part.parse::<u8>() {
            Ok(_) => {}
            Err(_) => anyhow::bail!("Invalid IP address: {}. Each octet must be 0-255.", ip),
        }
    }

    Ok(())
}
