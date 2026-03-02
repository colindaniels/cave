use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use crate::config::Config;
use crate::ssh::{self, SshConnection};
use crate::status::{get_node_status, NodeStatus};

pub async fn run(hostname: &str, image: &str) -> Result<()> {
    let config = Config::load()?;

    // Find the node
    let node = config.get_node(hostname)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found. Use 'cave list' to see registered nodes.", hostname))?;

    // Check if image exists
    let image_path = Config::images_dir().join(image);
    if !image_path.exists() {
        // Try with .iso extension
        let image_path_iso = Config::images_dir().join(format!("{}.iso", image));
        if !image_path_iso.exists() {
            anyhow::bail!(
                "Image '{}' not found in {:?}. Use 'cave images' to list available images.",
                image,
                Config::images_dir()
            );
        }
    }

    let image_path = if image_path.exists() {
        image_path
    } else {
        Config::images_dir().join(format!("{}.iso", image))
    };

    // Check node status
    println!("Checking node status...");
    let status = get_node_status(node);

    match status {
        NodeStatus::Offline => {
            anyhow::bail!("Node '{}' is offline. Cannot deploy.", hostname);
        }
        NodeStatus::Active => {
            println!("WARNING: Node '{}' has an active deployment. This will overwrite it.", hostname);
        }
        NodeStatus::Standby => {
            println!("Node '{}' is in standby mode. Ready for deployment.", hostname);
        }
    }

    // Connect via SSH
    println!("Connecting to {}...", node.ip);
    let ssh = SshConnection::connect(&node.ip)
        .context("Failed to connect to node via SSH")?;

    // Get image size for progress indication
    let image_size = std::fs::metadata(&image_path)?.len();
    let image_size_mb = image_size / (1024 * 1024);
    println!("Image size: {} MB", image_size_mb);

    // Find available drives
    println!("Detecting available drives...");
    let (drive_output, _) = ssh.execute_with_status(
        "ls -1 /dev/nvme*n1 /dev/sd[a-z] /dev/vd[a-z] /dev/mmcblk[0-9] 2>/dev/null | sort"
    )?;

    let drives: Vec<&str> = drive_output.lines().filter(|l| !l.is_empty()).collect();

    if drives.is_empty() {
        anyhow::bail!("No drives found on node");
    }

    let target_device = if drives.len() == 1 {
        println!("Found drive: {}", drives[0]);
        drives[0].to_string()
    } else {
        println!("\nAvailable drives:");
        for (i, drive) in drives.iter().enumerate() {
            // Get drive size
            let size_cmd = format!("cat /sys/block/{}/size 2>/dev/null", drive.trim_start_matches("/dev/"));
            let (size_out, _) = ssh.execute_with_status(&size_cmd).unwrap_or_default();
            let size_sectors: u64 = size_out.trim().parse().unwrap_or(0);
            let size_gb = (size_sectors * 512) / (1024 * 1024 * 1024);
            println!("  [{}] {} ({} GB)", i + 1, drive, size_gb);
        }

        print!("\nSelect drive [1-{}]: ", drives.len());
        std::io::Write::flush(&mut std::io::stdout())?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let selection: usize = input.trim().parse()
            .map_err(|_| anyhow::anyhow!("Invalid selection"))?;

        if selection < 1 || selection > drives.len() {
            anyhow::bail!("Selection out of range");
        }

        drives[selection - 1].to_string()
    };

    println!("Target drive: {}", target_device);

    // Transfer image to node
    println!("\nTransferring image to node...");
    let remote_path = "/tmp/deploy.iso";

    let pb = ProgressBar::new(image_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Use scp for transfer
    ssh::scp_file(&node.ip, &image_path, remote_path)
        .context("Failed to transfer image to node")?;

    pb.finish_with_message("Transfer complete");

    // Write image to drive
    println!("\nWriting image to drive...");
    println!("This may take several minutes...");

    let dd_command = format!(
        "dd if={} of={} bs=4M conv=fsync 2>&1",
        remote_path, target_device
    );

    let (output, status) = ssh.execute_with_status(&dd_command)?;
    println!("{}", output);

    if status != 0 {
        anyhow::bail!("dd command failed with status {}", status);
    }

    // Clean up
    println!("Cleaning up...");
    let _ = ssh.execute(&format!("rm -f {}", remote_path));

    // Sync and reboot
    println!("Syncing and rebooting...");
    let _ = ssh.execute("sync");

    // Give it a moment, then reboot
    let _ = ssh.execute("nohup sh -c 'sleep 2 && reboot' &");

    println!("\nDeployment complete!");
    println!("Node '{}' is rebooting into the new image.", hostname);
    println!("Use 'cave list' to check status after reboot.");

    Ok(())
}
