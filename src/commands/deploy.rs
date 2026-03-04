use anyhow::{Context, Result};
use dialoguer::{Confirm, Input};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::Config;
use crate::ssh::{self, SshConnection};
use crate::status::{get_node_status, NodeStatus};
use crate::vm;

pub async fn run(hostname: &str, image: &str, memory: u32, cpus: u32) -> Result<()> {
    let config = Config::load()?;

    // Find the node
    let node = config.get_node(hostname)
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found. Use 'cave list' to see registered nodes.", hostname))?;

    // Find the image
    let image_path = find_image(image)?;
    println!("Image: {:?}", image_path);

    // Check if this is a cloud image that needs cloud-init
    let seed_iso_path = if needs_cloud_init(&image_path) {
        Some(create_cloud_init_iso(hostname)?)
    } else {
        None
    };

    // Check node status
    println!("\nChecking node status...");
    let status = get_node_status(node);

    match status {
        NodeStatus::Offline => {
            anyhow::bail!("Node '{}' is offline. Cannot deploy.", hostname);
        }
        NodeStatus::Active => {
            println!("Node '{}' has an active VM. Stopping it first...", hostname);
        }
        NodeStatus::Standby => {
            println!("Node '{}' is in standby mode. Ready for deployment.", hostname);
        }
    }

    // Connect via SSH to the Alpine host
    println!("Connecting to {} (Alpine host)...", node.ip);
    let ssh = SshConnection::connect(&node.ip)
        .context("Failed to connect to node via SSH")?;

    // Set up hypervisor if needed
    vm::setup_hypervisor(&ssh)?;

    // Stop existing VM if running
    if vm::is_vm_running(&ssh, hostname)? {
        println!("Stopping existing VM...");
        vm::stop_vm(&ssh, hostname)?;
    }

    // Get image size for transfer
    let image_size = std::fs::metadata(&image_path)?.len();
    let image_size_mb = image_size / (1024 * 1024);
    println!("\nImage: {:?}", image_path);
    println!("Size: {} MB", image_size_mb);
    println!("VM Config: {} MB RAM, {} CPUs", memory, cpus);

    // Ensure destination directory exists
    let _ = ssh.execute(&format!("mkdir -p {}", vm::VM_IMAGES_PATH));

    // Transfer cloud image to node
    println!("\nTransferring image to node...");
    let remote_image_path = format!("{}/{}.qcow2", vm::VM_IMAGES_PATH, hostname);

    let pb = ProgressBar::new(image_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    ssh::scp_file(&node.ip, &image_path, &remote_image_path)
        .context("Failed to transfer image to node")?;

    pb.finish_with_message("Transfer complete");

    // Transfer seed ISO if we have one
    let remote_seed_path = if let Some(ref seed_path) = seed_iso_path {
        println!("Transferring cloud-init seed ISO...");
        let remote_seed = format!("{}/{}-seed.iso", vm::VM_IMAGES_PATH, hostname);
        ssh::scp_file(&node.ip, seed_path, &remote_seed)
            .context("Failed to transfer seed ISO")?;
        println!("  Seed ISO transferred");
        Some(remote_seed)
    } else {
        None
    };

    // Start the VM
    println!("\nStarting VM...");
    vm::start_vm(&ssh, hostname, &remote_image_path, remote_seed_path.as_deref(), memory, cpus)?;

    println!("\n=== Deployment Complete ===");
    println!("VM '{}' is now running on node.", hostname);
    println!("\nVM Details:");
    println!("  Host (Alpine): ssh root@{}", node.ip);
    println!("  Memory: {} MB", memory);
    println!("  CPUs: {}", cpus);
    println!("\nThe VM should get an IP via DHCP on your network.");
    println!("Check your router or run 'cave list' to find it.");
    println!("\nSSH access (after VM gets IP):");
    println!("  ssh root@<vm-ip>  (password: cave, or use your SSH key)");

    Ok(())
}

fn find_image(image: &str) -> Result<PathBuf> {
    let images_dir = Config::images_dir();

    // Check if absolute path
    if std::path::Path::new(image).is_absolute() {
        let path = PathBuf::from(image);
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!("Image not found: {}", image);
    }

    // Check in images directory
    let path = images_dir.join(image);
    if path.exists() {
        return Ok(path);
    }

    // Try common extensions
    let extensions = ["qcow2", "img", "iso"];
    for ext in extensions {
        let p = images_dir.join(format!("{}.{}", image, ext));
        if p.exists() {
            return Ok(p);
        }
    }

    anyhow::bail!(
        "Image '{}' not found in {:?}. Use 'cave images' to list available images.",
        image,
        images_dir
    )
}

fn needs_cloud_init(image_path: &std::path::Path) -> bool {
    let filename = image_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // Cloud images typically have these patterns
    filename.contains("cloudimg") ||
    filename.contains("cloud") ||
    filename.contains("generic") ||
    (filename.ends_with(".img") && !filename.contains("disk")) ||
    (filename.ends_with(".qcow2") && (filename.contains("ubuntu") || filename.contains("debian") || filename.contains("fedora")))
}

fn create_cloud_init_iso(hostname: &str) -> Result<PathBuf> {
    println!("\nThis is a cloud image. Creating cloud-init configuration...");

    // Prompt for customization
    let enable_password = Confirm::new()
        .with_prompt("Enable SSH password login?")
        .default(true)
        .interact()?;

    let root_password = if enable_password {
        Input::new()
            .with_prompt("Root password")
            .default("cave".to_string())
            .interact_text()?
    } else {
        "".to_string()
    };

    // Get SSH public key
    let ssh_pubkey_path = Config::ssh_public_key();
    if !ssh_pubkey_path.exists() {
        anyhow::bail!("SSH public key not found at {:?}. Run 'cave server init' first.", ssh_pubkey_path);
    }
    let ssh_pubkey = fs::read_to_string(&ssh_pubkey_path)
        .context("Failed to read SSH public key")?
        .trim()
        .to_string();

    // Create temp directory for cloud-init files
    let seed_dir = Config::images_dir().join(format!("{}-seed", hostname));
    if seed_dir.exists() {
        fs::remove_dir_all(&seed_dir)?;
    }
    fs::create_dir_all(&seed_dir)?;

    // Create user-data
    let user_data_path = seed_dir.join("user-data");
    let mut user_data = String::from("#cloud-config\n");

    // Users configuration
    user_data.push_str("users:\n");
    user_data.push_str("  - name: root\n");
    user_data.push_str("    lock_passwd: false\n");
    user_data.push_str("    ssh_authorized_keys:\n");
    user_data.push_str(&format!("      - {}\n", ssh_pubkey));

    // SSH settings
    user_data.push_str(&format!("ssh_pwauth: {}\n", enable_password));

    // Password configuration
    if enable_password && !root_password.is_empty() {
        user_data.push_str("chpasswd:\n");
        user_data.push_str(&format!("  list: |\n    root:{}\n", root_password));
        user_data.push_str("  expire: false\n");
    }

    // Disable cloud-init after first boot (faster subsequent boots)
    user_data.push_str("runcmd:\n");
    user_data.push_str("  - touch /etc/cloud/cloud-init.disabled\n");

    fs::write(&user_data_path, &user_data)?;

    // Create meta-data
    let meta_data_path = seed_dir.join("meta-data");
    let meta_data = format!(
        "instance-id: {}\nlocal-hostname: {}\n",
        hostname, hostname
    );
    fs::write(&meta_data_path, &meta_data)?;

    // Create the seed ISO
    let seed_iso_path = Config::images_dir().join(format!("{}-seed.iso", hostname));

    println!("  Creating seed ISO...");

    // Try cloud-localds first (from cloud-image-utils)
    let result = std::process::Command::new("cloud-localds")
        .arg(&seed_iso_path)
        .arg(&user_data_path)
        .arg(&meta_data_path)
        .output();

    if let Ok(output) = result {
        if output.status.success() {
            println!("  Seed ISO created with cloud-localds");
            // Clean up temp files
            let _ = fs::remove_dir_all(&seed_dir);
            return Ok(seed_iso_path);
        }
    }

    // Fallback to genisoimage/mkisofs
    let iso_tool = if std::process::Command::new("genisoimage").arg("--version").output().is_ok() {
        "genisoimage"
    } else if std::process::Command::new("mkisofs").arg("--version").output().is_ok() {
        "mkisofs"
    } else {
        anyhow::bail!(
            "Neither cloud-localds nor genisoimage/mkisofs found.\n\
            Install with: sudo pacman -S cloud-image-utils\n\
            Or: sudo pacman -S cdrtools"
        );
    };

    let output = std::process::Command::new(iso_tool)
        .args([
            "-output", seed_iso_path.to_str().unwrap(),
            "-volid", "CIDATA",
            "-joliet",
            "-rock",
            user_data_path.to_str().unwrap(),
            meta_data_path.to_str().unwrap(),
        ])
        .output()
        .context("Failed to create seed ISO")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to create seed ISO:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("  Seed ISO created with {}", iso_tool);

    // Clean up temp files
    let _ = fs::remove_dir_all(&seed_dir);

    Ok(seed_iso_path)
}
