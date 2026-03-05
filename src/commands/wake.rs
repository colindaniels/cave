use anyhow::{Context, Result};
use console::style;
use std::net::UdpSocket;

use crate::config::Config;
use crate::ui;

pub async fn run(hostname: &str) -> Result<()> {
    let config = Config::load()?;

    let node = config
        .get_node(hostname)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", hostname))?;

    // Parse MAC address
    let mac_bytes = parse_mac(&node.mac)?;

    // Build magic packet: 6 bytes of 0xFF + MAC repeated 16 times
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac_bytes);
    }

    // Send to broadcast address on port 9 (standard WoL port)
    let socket = UdpSocket::bind("0.0.0.0:0").context("Failed to create UDP socket")?;
    socket.set_broadcast(true).context("Failed to enable broadcast")?;
    socket
        .send_to(&packet, "255.255.255.255:9")
        .context("Failed to send magic packet")?;

    ui::print_success(&format!("Wake-on-LAN packet sent to {}", hostname));
    println!(
        "  {} {}",
        style("MAC:").dim(),
        style(&node.mac).cyan()
    );

    Ok(())
}

fn parse_mac(mac: &str) -> Result<[u8; 6]> {
    let bytes: Vec<u8> = mac
        .split(':')
        .map(|s| u8::from_str_radix(s, 16))
        .collect::<Result<Vec<_>, _>>()
        .context("Invalid MAC address format")?;

    if bytes.len() != 6 {
        anyhow::bail!("MAC address must have 6 bytes");
    }

    let mut arr = [0u8; 6];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}
