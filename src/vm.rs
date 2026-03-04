use anyhow::Result;

use crate::ssh::SshConnection;

/// Path where VM disk images are stored on the node
pub const VM_IMAGES_PATH: &str = "/var/lib/cave";
/// Path for VM PID files
pub const VM_RUN_PATH: &str = "/var/run/cave";

/// Set up the Alpine host as a hypervisor (install QEMU, configure bridge)
pub fn setup_hypervisor(ssh: &SshConnection) -> Result<()> {
    println!("Setting up hypervisor...");

    // Check if QEMU is already installed
    let (output, status) = ssh.execute_with_status("which qemu-system-x86_64 2>/dev/null")?;

    if status != 0 || output.trim().is_empty() {
        println!("  Installing QEMU packages...");

        // Add community repo if not present (QEMU is in community)
        let _ = ssh.execute(
            "grep -q community /etc/apk/repositories || echo 'https://dl-cdn.alpinelinux.org/alpine/v3.21/community' >> /etc/apk/repositories"
        );
        let _ = ssh.execute("apk update");

        // Install QEMU and dependencies
        let (install_output, install_status) = ssh.execute_with_status(
            "apk add --no-cache qemu-system-x86_64 qemu-img bridge"
        )?;

        if install_status != 0 {
            eprintln!("  Install output: {}", install_output);
            anyhow::bail!("Failed to install QEMU packages");
        }
    } else {
        println!("  QEMU already installed");
    }

    // Load KVM modules
    println!("  Loading KVM modules...");
    let _ = ssh.execute("modprobe kvm 2>/dev/null || true");
    let _ = ssh.execute("modprobe kvm_intel 2>/dev/null || modprobe kvm_amd 2>/dev/null || true");
    let _ = ssh.execute("modprobe tun 2>/dev/null || true");

    // Check if KVM is available
    let (_, kvm_status) = ssh.execute_with_status("test -e /dev/kvm")?;
    if kvm_status != 0 {
        println!("  WARNING: KVM not available, VMs will run without hardware acceleration");
    } else {
        println!("  KVM acceleration available");
    }

    // Create directories
    let _ = ssh.execute(&format!("mkdir -p {} {}", VM_IMAGES_PATH, VM_RUN_PATH));

    // Set up bridged networking
    setup_bridge(ssh)?;

    // Set up VM auto-start service
    setup_autostart(ssh)?;

    println!("  Hypervisor ready");
    Ok(())
}

/// Set up auto-start service for VMs on boot
fn setup_autostart(ssh: &SshConnection) -> Result<()> {
    // Create the startup script
    let startup_script = r#"#!/bin/sh
# Cave VM auto-start script
# Starts all VMs with saved configurations

VM_PATH="/var/lib/cave"
RUN_PATH="/var/run/cave"

# Ensure directories exist
mkdir -p "$RUN_PATH"

# Wait for network bridge to be ready
sleep 2

# Start each VM that has a config file
for conf in "$VM_PATH"/*.conf; do
    [ -f "$conf" ] || continue

    # Source the config
    . "$conf"

    # Check if disk exists
    [ -f "$DISK_PATH" ] || continue

    # Check if already running
    PID_FILE="$RUN_PATH/$VM_NAME.pid"
    if [ -f "$PID_FILE" ] && kill -0 $(cat "$PID_FILE") 2>/dev/null; then
        echo "VM $VM_NAME already running"
        continue
    fi

    echo "Starting VM: $VM_NAME"

    # Build seed drive option if seed ISO exists
    SEED_DRIVE=""
    if [ -n "$SEED_ISO" ] && [ -f "$SEED_ISO" ]; then
        SEED_DRIVE="-drive file=$SEED_ISO,format=raw,if=virtio,readonly=on"
    fi

    # Determine acceleration
    ACCEL=""
    [ -e /dev/kvm ] && ACCEL="-enable-kvm"

    # Generate MAC from VM name (simple hash)
    MAC=$(echo -n "$VM_NAME" | md5sum | sed 's/^\(..\)\(..\)\(..\)\(..\)\(..\).*$/02:\1:\2:\3:\4:\5/')

    # Start the VM
    qemu-system-x86_64 \
        $ACCEL \
        -m "$MEMORY_MB" \
        -smp "$CPUS" \
        -cpu host \
        -drive "file=$DISK_PATH,format=qcow2,if=virtio" \
        $SEED_DRIVE \
        -netdev bridge,id=net0,br=br0 \
        -device "virtio-net-pci,netdev=net0,mac=$MAC" \
        -serial "file:$RUN_PATH/$VM_NAME.log" \
        -display none \
        -daemonize \
        -pidfile "$PID_FILE" \
        -qmp "unix:$RUN_PATH/$VM_NAME.sock,server,nowait"

    if [ $? -eq 0 ]; then
        echo "  Started $VM_NAME"
    else
        echo "  Failed to start $VM_NAME"
    fi
done
"#;

    // Write the startup script
    let _ = ssh.execute(&format!(
        "cat > /usr/local/bin/cave-autostart << 'SCRIPT'\n{}SCRIPT\nchmod +x /usr/local/bin/cave-autostart",
        startup_script
    ));

    // Create OpenRC init script
    let init_script = r#"#!/sbin/openrc-run

name="cave-vms"
description="Cave VM Auto-start"
command="/usr/local/bin/cave-autostart"
command_background="no"

depend() {
    need net
    after bridge
}
"#;

    let _ = ssh.execute(&format!(
        "cat > /etc/init.d/cave-vms << 'INIT'\n{}INIT\nchmod +x /etc/init.d/cave-vms",
        init_script
    ));

    // Enable the service
    let _ = ssh.execute("rc-update add cave-vms default 2>/dev/null || true");

    Ok(())
}

/// Set up bridged networking on Alpine
fn setup_bridge(ssh: &SshConnection) -> Result<()> {
    // Configure QEMU bridge ACL
    let _ = ssh.execute("mkdir -p /etc/qemu && echo 'allow br0' > /etc/qemu/bridge.conf");

    // Check if bridge already exists
    let (output, _) = ssh.execute_with_status("ip link show br0 2>/dev/null")?;

    if output.contains("br0") {
        println!("  Bridge br0 already exists");
        return Ok(());
    }

    println!("  Setting up network bridge...");

    // Find the primary network interface
    let (iface_output, _) = ssh.execute_with_status(
        "ip route | grep default | awk '{print $5}' | head -1"
    )?;
    let primary_iface = iface_output.trim();

    if primary_iface.is_empty() {
        anyhow::bail!("Could not determine primary network interface");
    }

    println!("  Primary interface: {}", primary_iface);

    // Get current IP configuration
    let (ip_output, _) = ssh.execute_with_status(&format!(
        "ip -4 addr show {} | grep inet | awk '{{print $2}}'",
        primary_iface
    ))?;
    let current_ip = ip_output.trim();

    let (gw_output, _) = ssh.execute_with_status(
        "ip route | grep default | awk '{print $3}'"
    )?;
    let gateway = gw_output.trim();

    if current_ip.is_empty() || gateway.is_empty() {
        anyhow::bail!("Could not determine IP configuration");
    }

    println!("  Current IP: {}, Gateway: {}", current_ip, gateway);

    // Create bridge and migrate interface
    // This is done carefully to avoid losing connectivity
    let bridge_commands = format!(
        r#"
        ip link add name br0 type bridge
        ip link set br0 up
        ip link set {} master br0
        ip addr del {} dev {}
        ip addr add {} dev br0
        ip route add default via {}
        "#,
        primary_iface, current_ip, primary_iface, current_ip, gateway
    );

    // Execute bridge setup
    let (_, bridge_status) = ssh.execute_with_status(&bridge_commands)?;

    if bridge_status != 0 {
        // Try to recover
        let _ = ssh.execute(&format!("ip addr add {} dev {}", current_ip, primary_iface));
        let _ = ssh.execute(&format!("ip route add default via {}", gateway));
        anyhow::bail!("Failed to set up bridge, attempted recovery");
    }

    println!("  Bridge br0 configured");
    Ok(())
}

/// Start a VM
pub fn start_vm(
    ssh: &SshConnection,
    vm_name: &str,
    image_path: &str,
    seed_iso_path: Option<&str>,
    memory_mb: u32,
    cpus: u32,
    disk_size_gb: Option<u32>,
) -> Result<()> {
    let disk_path = format!("{}/{}.qcow2", VM_IMAGES_PATH, vm_name);
    let pid_file = format!("{}/{}.pid", VM_RUN_PATH, vm_name);
    let qmp_socket = format!("{}/{}.sock", VM_RUN_PATH, vm_name);

    // Check if VM is already running
    if is_vm_running(ssh, vm_name)? {
        anyhow::bail!("VM '{}' is already running", vm_name);
    }

    // Copy image to VM storage if it doesn't exist
    let (_, exists_status) = ssh.execute_with_status(&format!("test -f {}", disk_path))?;
    if exists_status != 0 {
        println!("  Creating VM disk from image...");
        // Create a copy-on-write overlay based on the source image
        let (_, cp_status) = ssh.execute_with_status(&format!(
            "cp {} {}",
            image_path, disk_path
        ))?;
        if cp_status != 0 {
            anyhow::bail!("Failed to copy image to VM storage");
        }

        // Resize disk if a size was specified
        if let Some(size_gb) = disk_size_gb {
            println!("  Resizing disk to {}GB...", size_gb);
            let (resize_output, resize_status) = ssh.execute_with_status(&format!(
                "qemu-img resize {} {}G",
                disk_path, size_gb
            ))?;
            if resize_status != 0 {
                anyhow::bail!("Failed to resize disk: {}", resize_output);
            }
        }
    }

    // Determine if KVM is available
    let (_, kvm_status) = ssh.execute_with_status("test -e /dev/kvm")?;
    let accel = if kvm_status == 0 { "-enable-kvm" } else { "" };

    // Build cloud-init seed ISO drive if provided
    let seed_drive = if let Some(seed_path) = seed_iso_path {
        format!("-drive file={},format=raw,if=virtio,readonly=on", seed_path)
    } else {
        String::new()
    };

    // Serial console log file
    let serial_log = format!("{}/{}.log", VM_RUN_PATH, vm_name);

    // Build QEMU command
    let qemu_cmd = format!(
        r#"qemu-system-x86_64 \
            {} \
            -m {} \
            -smp {} \
            -cpu host \
            -drive file={},format=qcow2,if=virtio \
            {} \
            -netdev bridge,id=net0,br=br0 \
            -device virtio-net-pci,netdev=net0,mac={} \
            -serial file:{} \
            -display none \
            -daemonize \
            -pidfile {} \
            -qmp unix:{},server,nowait"#,
        accel,
        memory_mb,
        cpus,
        disk_path,
        seed_drive,
        generate_mac(vm_name),
        serial_log,
        pid_file,
        qmp_socket
    );

    println!("  Starting VM...");
    let (output, status) = ssh.execute_with_status(&qemu_cmd)?;

    if status != 0 {
        anyhow::bail!("Failed to start VM: {}", output);
    }

    // Verify it's running
    std::thread::sleep(std::time::Duration::from_secs(1));
    if !is_vm_running(ssh, vm_name)? {
        anyhow::bail!("VM started but is not running - check logs");
    }

    // Save VM config for auto-start on reboot
    let seed_path_str = seed_iso_path.unwrap_or("");
    let config_content = format!(
        "VM_NAME={}\nMEMORY_MB={}\nCPUS={}\nDISK_PATH={}\nSEED_ISO={}\n",
        vm_name, memory_mb, cpus, disk_path, seed_path_str
    );
    let config_path = format!("{}/{}.conf", VM_IMAGES_PATH, vm_name);
    let _ = ssh.execute(&format!("cat > {} << 'EOF'\n{}EOF", config_path, config_content));

    println!("  VM '{}' started", vm_name);
    Ok(())
}

/// Stop a VM gracefully
pub fn stop_vm(ssh: &SshConnection, vm_name: &str) -> Result<()> {
    let pid_file = format!("{}/{}.pid", VM_RUN_PATH, vm_name);
    let qmp_socket = format!("{}/{}.sock", VM_RUN_PATH, vm_name);

    if !is_vm_running(ssh, vm_name)? {
        println!("  VM '{}' is not running", vm_name);
        return Ok(());
    }

    // Try graceful shutdown via QMP first
    println!("  Sending shutdown signal...");
    let (_, qmp_status) = ssh.execute_with_status(&format!(
        r#"echo '{{"execute":"qmp_capabilities"}}{{"execute":"system_powerdown"}}' | nc -U {} 2>/dev/null"#,
        qmp_socket
    ))?;

    if qmp_status == 0 {
        // Wait for graceful shutdown
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if !is_vm_running(ssh, vm_name)? {
                println!("  VM '{}' stopped gracefully", vm_name);
                cleanup_vm_files(ssh, vm_name)?;
                return Ok(());
            }
        }
    }

    // Force kill if graceful shutdown failed
    println!("  Force stopping VM...");
    let (pid_content, _) = ssh.execute_with_status(&format!("cat {} 2>/dev/null", pid_file))?;
    let pid = pid_content.trim();

    if !pid.is_empty() {
        let _ = ssh.execute(&format!("kill -9 {} 2>/dev/null || true", pid));
    }

    cleanup_vm_files(ssh, vm_name)?;
    println!("  VM '{}' stopped", vm_name);
    Ok(())
}

/// Clean up VM runtime files (not the disk)
fn cleanup_vm_files(ssh: &SshConnection, vm_name: &str) -> Result<()> {
    let pid_file = format!("{}/{}.pid", VM_RUN_PATH, vm_name);
    let qmp_socket = format!("{}/{}.sock", VM_RUN_PATH, vm_name);

    let _ = ssh.execute(&format!("rm -f {} {}", pid_file, qmp_socket));
    Ok(())
}

/// Check if a VM is running
pub fn is_vm_running(ssh: &SshConnection, vm_name: &str) -> Result<bool> {
    let pid_file = format!("{}/{}.pid", VM_RUN_PATH, vm_name);

    let (output, status) = ssh.execute_with_status(&format!(
        "test -f {} && kill -0 $(cat {}) 2>/dev/null && echo running",
        pid_file, pid_file
    ))?;

    Ok(status == 0 && output.trim() == "running")
}

/// Get VM info (for status display)
pub fn get_vm_info(ssh: &SshConnection, vm_name: &str) -> Result<Option<VmInfo>> {
    if !is_vm_running(ssh, vm_name)? {
        return Ok(None);
    }

    let pid_file = format!("{}/{}.pid", VM_RUN_PATH, vm_name);

    // Get PID
    let (pid_output, _) = ssh.execute_with_status(&format!("cat {}", pid_file))?;
    let pid = pid_output.trim().to_string();

    // Get memory/CPU from process
    let (_ps_output, _) = ssh.execute_with_status(&format!(
        "ps -p {} -o %mem,%cpu --no-headers 2>/dev/null || echo '0 0'",
        pid
    ))?;

    Ok(Some(VmInfo {
        name: vm_name.to_string(),
        pid,
        status: "running".to_string(),
    }))
}

/// Delete a VM (stop if running and remove disk, config, and seed ISO)
pub fn delete_vm(ssh: &SshConnection, vm_name: &str) -> Result<()> {
    // Stop if running
    if is_vm_running(ssh, vm_name)? {
        stop_vm(ssh, vm_name)?;
    }

    // Remove disk, config, and seed ISO
    let disk_path = format!("{}/{}.qcow2", VM_IMAGES_PATH, vm_name);
    let config_path = format!("{}/{}.conf", VM_IMAGES_PATH, vm_name);
    let seed_path = format!("{}/{}-seed.iso", VM_IMAGES_PATH, vm_name);
    let _ = ssh.execute(&format!("rm -f {} {} {}", disk_path, config_path, seed_path));

    println!("  VM '{}' deleted", vm_name);
    Ok(())
}

/// Generate a deterministic MAC address for lookup (public version)
pub fn generate_mac_for_lookup(vm_name: &str) -> String {
    generate_mac(vm_name)
}

/// Generate a deterministic MAC address based on VM name
fn generate_mac(vm_name: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    vm_name.hash(&mut hasher);
    let hash = hasher.finish();

    // Use locally administered, unicast MAC (02:xx:xx:xx:xx:xx)
    format!(
        "02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        (hash >> 8) as u8,
        (hash >> 16) as u8,
        (hash >> 24) as u8,
        (hash >> 32) as u8,
        (hash >> 40) as u8,
    )
}

#[derive(Debug)]
pub struct VmInfo {
    pub name: String,
    pub pid: String,
    pub status: String,
}
