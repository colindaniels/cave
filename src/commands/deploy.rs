use anyhow::{Context, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect, Input};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::commands::images::get_image_display_name;
use crate::config::{scan_for_ip, Config};
use crate::ssh::{self, SshConnection};
use crate::status::{get_disk_info, get_node_resources, get_node_status, DiskInfo, NodeStatus};
use crate::ui;
use crate::vm;

pub async fn run(hostname: Option<&str>, image: Option<&str>) -> Result<()> {
    let config = Config::load()?;
    let theme = ColorfulTheme::default();

    // Check if we have any nodes
    if config.nodes.is_empty() {
        ui::print_error("No nodes registered");
        println!(
            "  Run {} to add a node first",
            style("cave init <hostname> <mac>").cyan()
        );
        return Ok(());
    }

    // Select or validate node
    let node = if let Some(h) = hostname {
        config
            .get_node(h)
            .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", h))?
            .clone()
    } else {
        // Interactive node selection
        let node_names: Vec<String> = config
            .nodes
            .iter()
            .map(|n| format!("{} ({})", n.hostname, n.mac))
            .collect();

        let selection = FuzzySelect::with_theme(&theme)
            .with_prompt("Select node")
            .items(&node_names)
            .default(0)
            .interact()?;

        config.nodes[selection].clone()
    };

    // Scan network for node IP
    let node_ip = scan_for_ip(&node.mac).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not find node '{}' on network. Is it powered on?",
            node.hostname
        )
    })?;

    // Get available images
    let images = get_available_images()?;
    if images.is_empty() {
        ui::print_error("No images available");
        println!(
            "  Run {} to download an image",
            style("cave image pull <url>").cyan()
        );
        return Ok(());
    }

    // Select or validate image
    let image_path = if let Some(img) = image {
        find_image(img)?
    } else {
        // Interactive image selection
        let image_names: Vec<String> = images
            .iter()
            .map(|(name, size)| {
                let display_name = get_image_display_name(name);
                if display_name == *name {
                    // Not a known cloud image, show filename with size
                    format!("{} ({})", name, ui::format_size(*size))
                } else {
                    // Known cloud image, show friendly name (already includes size info)
                    display_name
                }
            })
            .collect();

        let selection = FuzzySelect::with_theme(&theme)
            .with_prompt("Select image")
            .items(&image_names)
            .default(0)
            .interact()?;

        Config::images_dir().join(&images[selection].0)
    };

    // VM name
    let vm_name: String = Input::with_theme(&theme)
        .with_prompt("VM name")
        .default(node.hostname.clone())
        .interact_text()?;

    // Connect to node to get disk info and resources
    let spinner = create_spinner("Checking node resources...");
    let ssh = SshConnection::connect(&node_ip).context("Failed to connect to node")?;
    let disks = get_disk_info(&ssh);
    let (node_ram_mb, node_cpu_cores) = get_node_resources(&ssh);
    drop(ssh); // Release connection for now
    spinner.finish_and_clear();

    // Show node resources
    println!("  {} RAM: {} MB, CPUs: {}",
        style("Node:").dim(),
        node_ram_mb,
        node_cpu_cores
    );

    // Select disk if multiple available
    let selected_disk: Option<&DiskInfo> = if disks.is_empty() {
        ui::print_warning("No disks detected on node - using default storage");
        None
    } else if disks.len() == 1 {
        println!("  {} {} {} ({})",
            style("Disk:").dim(),
            disks[0].name,
            format_disk_size(disks[0].size_bytes),
            disks[0].disk_type
        );
        Some(&disks[0])
    } else {
        // Multiple disks - let user choose
        let disk_names: Vec<String> = disks
            .iter()
            .map(|d| format!("{} - {} {} ", d.name, format_disk_size(d.size_bytes), d.disk_type))
            .collect();

        let disk_idx = FuzzySelect::with_theme(&theme)
            .with_prompt("Select disk for VM storage")
            .items(&disk_names)
            .default(0)
            .interact()?;

        Some(&disks[disk_idx])
    };

    // Memory selection - filter by available RAM (reserve ~512MB for host)
    let available_ram = node_ram_mb.saturating_sub(512);
    let all_memory_options = [
        (1024u32, "1024 MB (1 GB)"),
        (2048, "2048 MB (2 GB)"),
        (4096, "4096 MB (4 GB)"),
        (8192, "8192 MB (8 GB)"),
        (16384, "16384 MB (16 GB)"),
        (32768, "32768 MB (32 GB)"),
        (65536, "65536 MB (64 GB)"),
    ];

    let filtered_memory: Vec<_> = all_memory_options
        .iter()
        .filter(|(val, _)| *val <= available_ram)
        .collect();

    if filtered_memory.is_empty() {
        ui::print_error(&format!("Node has insufficient RAM ({} MB available)", available_ram));
        return Ok(());
    }

    let memory_labels: Vec<&str> = filtered_memory.iter().map(|(_, label)| *label).collect();
    let memory_values: Vec<u32> = filtered_memory.iter().map(|(val, _)| *val).collect();

    // Default to ~half of available options or 2GB
    let default_mem_idx = memory_values.iter().position(|&v| v >= 2048).unwrap_or(0);

    let memory_idx = FuzzySelect::with_theme(&theme)
        .with_prompt("Memory")
        .items(&memory_labels)
        .default(default_mem_idx)
        .interact()?;
    let memory = memory_values[memory_idx];

    // CPU selection - filter by available cores
    let all_cpu_options = [
        (1u32, "1 CPU"),
        (2, "2 CPUs"),
        (4, "4 CPUs"),
        (8, "8 CPUs"),
        (16, "16 CPUs"),
        (32, "32 CPUs"),
    ];

    let filtered_cpus: Vec<_> = all_cpu_options
        .iter()
        .filter(|(val, _)| *val <= node_cpu_cores)
        .collect();

    if filtered_cpus.is_empty() {
        ui::print_error("Node has no CPU cores available");
        return Ok(());
    }

    let cpu_labels: Vec<&str> = filtered_cpus.iter().map(|(_, label)| *label).collect();
    let cpu_values: Vec<u32> = filtered_cpus.iter().map(|(val, _)| *val).collect();

    // Default to 2 CPUs or half of available
    let default_cpu_idx = cpu_values.iter().position(|&v| v >= 2).unwrap_or(0);

    let cpu_idx = FuzzySelect::with_theme(&theme)
        .with_prompt("CPUs")
        .items(&cpu_labels)
        .default(default_cpu_idx)
        .interact()?;
    let cpus = cpu_values[cpu_idx];

    // Generate disk size options based on selected disk
    let (disk_options, disk_values) = generate_disk_options(selected_disk);

    let disk_idx = FuzzySelect::with_theme(&theme)
        .with_prompt("Disk size")
        .items(&disk_options)
        .default(disk_options.len().min(3).saturating_sub(1)) // Default to middle option
        .interact()?;
    let disk_size = disk_values[disk_idx];

    // Show summary
    println!();
    let disk_display = disk_size.map_or("Default".to_string(), |s| format!("{} GB", s));
    ui::print_box("Deployment Summary", &[
        ("Node", &format!("{} ({})", node.hostname, node_ip)),
        ("Image", image_path.file_name().unwrap().to_str().unwrap()),
        ("VM Name", &vm_name),
        ("Memory", &ui::format_memory(memory)),
        ("CPUs", &cpus.to_string()),
        ("Disk", &disk_display),
    ]);

    // Confirm deployment
    let proceed = Confirm::with_theme(&theme)
        .with_prompt("Deploy VM?")
        .default(true)
        .interact()?;

    if !proceed {
        println!("{}", style("Deployment cancelled").dim());
        return Ok(());
    }

    println!();

    // Check if this is a cloud image that needs cloud-init
    let seed_iso_path = if needs_cloud_init(&image_path) {
        Some(create_cloud_init_iso(&vm_name, &theme)?)
    } else {
        None
    };

    // Check node status
    let spinner = create_spinner("Checking node status...");
    let status = get_node_status(&node_ip);
    spinner.finish_and_clear();

    match status {
        NodeStatus::Offline => {
            ui::print_error(&format!("Node '{}' is offline", node.hostname));
            return Ok(());
        }
        NodeStatus::Active => {
            ui::print_warning(&format!("Node '{}' has an active VM - stopping it first", node.hostname));
        }
        NodeStatus::Standby => {
            ui::print_success(&format!("Node '{}' is ready", node.hostname));
        }
    }

    // Remove existing VM config so watcher doesn't restart VM during deployment
    let vm_config_path = Config::vms_dir().join(format!("{}.conf", node.hostname));
    let _ = fs::remove_file(&vm_config_path);

    // Connect via SSH
    let spinner = create_spinner(&format!("Connecting to {}...", node_ip));
    let ssh = SshConnection::connect(&node_ip).context("Failed to connect via SSH")?;
    spinner.finish_and_clear();
    ui::print_success("Connected");

    // Set up hypervisor
    let spinner = create_spinner("Setting up hypervisor...");
    vm::setup_hypervisor(&ssh)?;
    spinner.finish_and_clear();
    ui::print_success("Hypervisor ready");

    // Mount storage disk
    let disk_name = selected_disk.map(|d| d.name.clone());
    if let Some(ref name) = disk_name {
        let spinner = create_spinner(&format!("Mounting storage ({})...", name));
        vm::mount_storage(&ssh, name)?;
        spinner.finish_and_clear();
        ui::print_success("Storage mounted");
    }

    // Stop existing VM if running
    if vm::is_vm_running(&ssh, &vm_name)? {
        let spinner = create_spinner("Stopping existing VM...");
        vm::stop_vm(&ssh, &vm_name)?;
        spinner.finish_and_clear();
        ui::print_success("Previous VM stopped");
    }

    // Ensure destination directory exists
    let _ = ssh.execute(&format!("mkdir -p {}", vm::VM_IMAGES_PATH));

    // Transfer image
    let image_size = std::fs::metadata(&image_path)?.len();
    let remote_image_path = format!("{}/{}.qcow2", vm::VM_IMAGES_PATH, vm_name);

    println!(
        "\n{} Transferring image {}",
        style("→").cyan().bold(),
        style(ui::format_size(image_size)).dim()
    );

    let pb = ProgressBar::new(image_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("━━╸"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    ssh::scp_file(&node_ip, &image_path, &remote_image_path)
        .context("Failed to transfer image")?;

    pb.finish_and_clear();
    ui::print_success("Image transferred");

    // Resize disk if a size was specified
    if let Some(size_gb) = disk_size {
        let spinner = create_spinner(&format!("Resizing disk to {}GB...", size_gb));
        let (output, status) = ssh.execute_with_status(&format!(
            "qemu-img resize {} {}G",
            remote_image_path, size_gb
        ))?;
        spinner.finish_and_clear();
        if status != 0 {
            ui::print_warning(&format!("Disk resize failed: {}", output.trim()));
        } else {
            ui::print_success(&format!("Disk resized to {}GB", size_gb));
        }
    }

    // Transfer seed ISO if needed
    let remote_seed_path = if let Some(ref seed_path) = seed_iso_path {
        let spinner = create_spinner("Transferring cloud-init seed...");
        let remote_seed = format!("{}/{}-seed.iso", vm::VM_IMAGES_PATH, vm_name);
        ssh::scp_file(&node_ip, seed_path, &remote_seed)
            .context("Failed to transfer seed ISO")?;
        spinner.finish_and_clear();
        ui::print_success("Cloud-init seed ready");
        Some(remote_seed)
    } else {
        None
    };

    // Start the VM
    let spinner = create_spinner("Starting VM...");
    vm::start_vm(&ssh, &vm_name, &remote_image_path, remote_seed_path.as_deref(), memory, cpus, disk_size)?;
    spinner.finish_and_clear();
    ui::print_success("VM started");

    // Save VM config on server for auto-start after node reboots
    let vms_dir = Config::vms_dir();
    fs::create_dir_all(&vms_dir)?;
    let vm_config_path = vms_dir.join(format!("{}.conf", node.hostname));
    let remote_seed_str = remote_seed_path.as_deref().unwrap_or("");
    let disk_name_str = disk_name.as_deref().unwrap_or("");
    let mac_addr = vm::generate_mac_for_lookup(&vm_name);
    let vm_config = format!(
        "NODE_IP={}\nVM_NAME={}\nMEMORY_MB={}\nCPUS={}\nDISK_PATH={}\nSEED_ISO={}\nDISK_NAME={}\nMAC={}\n",
        node_ip, vm_name, memory, cpus, remote_image_path, remote_seed_str, disk_name_str, mac_addr
    );
    fs::write(&vm_config_path, &vm_config)?;
    ui::print_success("VM config saved");

    // Success message
    ui::print_completion("Deployment Complete");

    println!();
    println!(
        "  {} {} is running on {}",
        style("VM").bold(),
        style(&vm_name).cyan(),
        style(&node.hostname).cyan()
    );
    println!();
    println!("  {} {}", style("Alpine host:").dim(), format!("ssh root@{}", node_ip));
    let disk_info = disk_size.map_or("default".to_string(), |s| format!("{}GB", s));
    println!("  {} {}", style("Config:").dim(), format!("{}, {} CPUs, {} disk", ui::format_memory(memory), cpus, disk_info));
    println!();
    println!(
        "  {} {}",
        style("Tip:").yellow(),
        "Run 'cave list' to see the VM's IP once it boots"
    );

    Ok(())
}

fn get_available_images() -> Result<Vec<(String, u64)>> {
    let images_dir = Config::images_dir();
    if !images_dir.exists() {
        return Ok(Vec::new());
    }

    let mut images = Vec::new();
    for entry in fs::read_dir(&images_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Skip seed ISOs and VM disk images
                if name.ends_with("-seed.iso")
                    || name.ends_with("-seed")
                    || name.contains(".cave.")
                    || name.ends_with(".qcow2")
                {
                    continue;
                }
                let size = fs::metadata(&path)?.len();
                images.push((name.to_string(), size));
            }
        }
    }

    // Sort by name
    images.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(images)
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
        "Image '{}' not found. Run {} to see available images.",
        image,
        style("cave images").cyan()
    )
}

fn needs_cloud_init(image_path: &std::path::Path) -> bool {
    let filename = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    filename.contains("cloudimg")
        || filename.contains("cloud")
        || filename.contains("generic")
        || (filename.ends_with(".img") && !filename.contains("disk"))
        || (filename.ends_with(".qcow2")
            && (filename.contains("ubuntu")
                || filename.contains("debian")
                || filename.contains("fedora")))
}

fn create_cloud_init_iso(hostname: &str, theme: &ColorfulTheme) -> Result<PathBuf> {
    println!();
    ui::print_header("Cloud-Init Configuration");

    let enable_password = Confirm::with_theme(theme)
        .with_prompt("Enable SSH password login?")
        .default(true)
        .interact()?;

    let root_password = if enable_password {
        Input::with_theme(theme)
            .with_prompt("Root password")
            .default("cave".to_string())
            .interact_text()?
    } else {
        "".to_string()
    };

    // Get SSH public key
    let ssh_pubkey_path = Config::ssh_public_key();
    if !ssh_pubkey_path.exists() {
        anyhow::bail!(
            "SSH public key not found. Run {} first.",
            style("cave server init").cyan()
        );
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

    user_data.push_str("users:\n");
    user_data.push_str("  - name: root\n");
    user_data.push_str("    lock_passwd: false\n");
    user_data.push_str("    ssh_authorized_keys:\n");
    user_data.push_str(&format!("      - {}\n", ssh_pubkey));

    user_data.push_str(&format!("ssh_pwauth: {}\n", enable_password));

    if enable_password && !root_password.is_empty() {
        user_data.push_str("chpasswd:\n");
        user_data.push_str(&format!("  list: |\n    root:{}\n", root_password));
        user_data.push_str("  expire: false\n");
    }

    user_data.push_str("runcmd:\n");
    user_data.push_str("  - touch /etc/cloud/cloud-init.disabled\n");

    fs::write(&user_data_path, &user_data)?;

    // Create meta-data
    let meta_data_path = seed_dir.join("meta-data");
    let meta_data = format!("instance-id: {}\nlocal-hostname: {}\n", hostname, hostname);
    fs::write(&meta_data_path, &meta_data)?;

    // Create the seed ISO
    let seed_iso_path = Config::images_dir().join(format!("{}-seed.iso", hostname));

    let spinner = create_spinner("Creating seed ISO...");

    // Try cloud-localds first
    let result = std::process::Command::new("cloud-localds")
        .arg(&seed_iso_path)
        .arg(&user_data_path)
        .arg(&meta_data_path)
        .output();

    if let Ok(output) = result {
        if output.status.success() {
            spinner.finish_and_clear();
            ui::print_success("Seed ISO created");
            let _ = fs::remove_dir_all(&seed_dir);
            return Ok(seed_iso_path);
        }
    }

    // Fallback to genisoimage/mkisofs
    let iso_tool = if std::process::Command::new("genisoimage")
        .arg("--version")
        .output()
        .is_ok()
    {
        "genisoimage"
    } else if std::process::Command::new("mkisofs")
        .arg("--version")
        .output()
        .is_ok()
    {
        "mkisofs"
    } else {
        spinner.finish_and_clear();
        anyhow::bail!(
            "cloud-localds or genisoimage required.\n  Install: {}",
            style("sudo pacman -S cloud-image-utils").cyan()
        );
    };

    let output = std::process::Command::new(iso_tool)
        .args([
            "-output",
            seed_iso_path.to_str().unwrap(),
            "-volid",
            "CIDATA",
            "-joliet",
            "-rock",
            user_data_path.to_str().unwrap(),
            meta_data_path.to_str().unwrap(),
        ])
        .output()
        .context("Failed to create seed ISO")?;

    if !output.status.success() {
        spinner.finish_and_clear();
        anyhow::bail!(
            "Failed to create seed ISO:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    spinner.finish_and_clear();
    ui::print_success("Seed ISO created");

    let _ = fs::remove_dir_all(&seed_dir);

    Ok(seed_iso_path)
}

fn create_spinner(message: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    spinner.set_message(message.to_string());
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner
}

fn format_disk_size(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const TB: u64 = 1_000_000_000_000;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else {
        format!("{} GB", bytes / GB)
    }
}

fn generate_disk_options(disk: Option<&DiskInfo>) -> (Vec<String>, Vec<Option<u32>>) {
    const GB: u64 = 1_000_000_000;

    let max_gb = disk
        .map(|d| (d.size_bytes / GB) as u32)
        .unwrap_or(500); // Default to 500GB max if no disk info

    let mut options = vec!["Default (image size)".to_string()];
    let mut values: Vec<Option<u32>> = vec![None];

    // Generate options based on available disk size
    let sizes = [10, 20, 50, 100, 200, 500, 1000];
    for &size in &sizes {
        if size <= max_gb {
            options.push(format!("{} GB", size));
            values.push(Some(size));
        }
    }

    // Add max size option if it's not already in the list
    if max_gb > 10 && !sizes.contains(&max_gb) && max_gb < 1000 {
        options.push(format!("{} GB (max)", max_gb));
        values.push(Some(max_gb));
    }

    (options, values)
}
