use anyhow::Result;
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm};

use crate::config::Config;
use crate::ssh;
use crate::ui;

pub async fn run(hostname: &str) -> Result<()> {
    let mut config = Config::load()?;
    let theme = ColorfulTheme::default();

    // Check if node exists
    if config.get_node(hostname).is_none() {
        ui::print_error(&format!("Node '{}' not found", hostname));
        return Ok(());
    }

    // Confirm removal
    let confirm = Confirm::with_theme(&theme)
        .with_prompt(format!("Remove node '{}'?", hostname))
        .default(false)
        .interact()?;

    if !confirm {
        println!("{}", style("Cancelled").dim());
        return Ok(());
    }

    // Remove from config
    config.remove_node(hostname)?;
    config.save()?;

    // Remove from SSH config
    ssh::remove_ssh_config(hostname)?;

    ui::print_success(&format!("Node '{}' removed", hostname));

    Ok(())
}
