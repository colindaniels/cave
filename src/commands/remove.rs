use anyhow::Result;

use crate::config::Config;
use crate::ssh;

pub async fn run(hostname: &str) -> Result<()> {
    let mut config = Config::load()?;

    // Remove from config
    config.remove_node(hostname)?;
    config.save()?;

    // Remove from SSH config
    ssh::remove_ssh_config(hostname)?;

    println!("Node '{}' removed successfully", hostname);

    Ok(())
}
