use crate::config::Node;
use crate::ssh::SshConnection;
use crate::vm;

#[derive(Debug, Clone, PartialEq)]
pub enum NodeStatus {
    Offline,
    Standby,
    Active,
}

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub name: String,       // e.g., "nvme0n1", "sda"
    pub size_bytes: u64,    // Total size in bytes
    pub available_bytes: Option<u64>, // Available space if mounted
    pub disk_type: String,  // "SSD" or "HDD"
    pub model: String,      // Disk model name
    pub mount_point: Option<String>, // Where it's mounted, if at all
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeStatus::Offline => write!(f, "offline"),
            NodeStatus::Standby => write!(f, "standby"),
            NodeStatus::Active => write!(f, "active"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeSpecs {
    pub cpu: String,
    pub cores: String,
    pub ram: String,
    pub gpu: String,
    pub disks: Vec<DiskInfo>,
}

impl Default for NodeSpecs {
    fn default() -> Self {
        Self {
            cpu: "N/A".to_string(),
            cores: "N/A".to_string(),
            ram: "N/A".to_string(),
            gpu: "N/A".to_string(),
            disks: Vec::new(),
        }
    }
}

/// Get raw node specs (RAM in MB, CPU cores) for resource bounding
pub fn get_node_resources(ssh: &SshConnection) -> (u32, u32) {
    let mut ram_mb: u32 = 8192; // Default 8GB
    let mut cpu_cores: u32 = 4; // Default 4 cores

    // Get RAM in MB
    if let Ok(output) = ssh.execute("free -m | grep Mem | awk '{print $2}'") {
        if let Ok(ram) = output.trim().parse::<u32>() {
            ram_mb = ram;
        }
    }

    // Get CPU cores
    if let Ok(output) = ssh.execute("nproc") {
        if let Ok(cores) = output.trim().parse::<u32>() {
            cpu_cores = cores;
        }
    }

    (ram_mb, cpu_cores)
}

/// Get disk information from a node via SSH
pub fn get_disk_info(ssh: &SshConnection) -> Vec<DiskInfo> {
    let mut disks = Vec::new();

    // Get list of block devices (disks only, not partitions)
    // Format: NAME|SIZE_BLOCKS|ROTATIONAL|MODEL
    let cmd = r#"
        for disk in /sys/block/*/; do
            name=$(basename "$disk")
            # Skip loop, ram, dm devices
            case "$name" in
                loop*|ram*|dm-*) continue ;;
            esac
            # Get size in 512-byte blocks
            size=$(cat "$disk/size" 2>/dev/null || echo 0)
            # Get rotational (0=SSD, 1=HDD)
            rot=$(cat "$disk/queue/rotational" 2>/dev/null || echo 1)
            # Get model
            model=$(cat "$disk/device/model" 2>/dev/null | tr -d '\n' || echo "Unknown")
            echo "$name|$size|$rot|$model"
        done
    "#;

    if let Ok(output) = ssh.execute(cmd) {
        for line in output.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                let name = parts[0].to_string();
                let size_blocks: u64 = parts[1].parse().unwrap_or(0);
                let size_bytes = size_blocks * 512; // Convert 512-byte blocks to bytes
                let rotational: u32 = parts[2].parse().unwrap_or(1);
                let model = parts[3].trim().to_string();

                // Skip small devices (< 1GB)
                if size_bytes < 1_000_000_000 {
                    continue;
                }

                let disk_type = if rotational == 0 { "SSD" } else { "HDD" }.to_string();

                // Check if any partition of this disk is mounted and get available space
                let (mount_point, available_bytes) = get_disk_mount_info(ssh, &name);

                disks.push(DiskInfo {
                    name,
                    size_bytes,
                    available_bytes,
                    disk_type,
                    model,
                    mount_point,
                });
            }
        }
    }

    // Sort by size descending
    disks.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    disks
}

/// Get mount point and available space for a disk
fn get_disk_mount_info(ssh: &SshConnection, disk_name: &str) -> (Option<String>, Option<u64>) {
    // Check df output for any partition of this disk
    let cmd = format!(
        "df -B1 2>/dev/null | grep '/dev/{}' | head -1 | awk '{{print $4\"|\"$6}}'",
        disk_name
    );

    if let Ok(output) = ssh.execute(&cmd) {
        let trimmed = output.trim();
        if !trimmed.is_empty() {
            let parts: Vec<&str> = trimmed.split('|').collect();
            if parts.len() >= 2 {
                let available: u64 = parts[0].parse().unwrap_or(0);
                let mount = parts[1].to_string();
                return (Some(mount), Some(available));
            }
        }
    }

    (None, None)
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub hostname: String,
    pub ip: String,
    pub mac: String,
    pub status: NodeStatus,
    pub specs: NodeSpecs,
    pub vm_info: Option<VmStatus>,
}

#[derive(Debug, Clone)]
pub struct VmStatus {
    pub running: bool,
    pub pid: Option<String>,
}

pub fn get_node_status(node: &Node) -> NodeStatus {
    match SshConnection::connect(&node.ip) {
        Ok(ssh) => {
            // Check if any VM is running on this node
            match is_any_vm_running(&ssh) {
                Ok(true) => NodeStatus::Active,
                Ok(false) => NodeStatus::Standby,
                Err(_) => NodeStatus::Standby,
            }
        }
        Err(_) => NodeStatus::Offline,
    }
}

/// Check if any VM is running on the node by looking for active PID files
fn is_any_vm_running(ssh: &SshConnection) -> anyhow::Result<bool> {
    let (output, _) = ssh.execute_with_status(&format!(
        "for pid in {}/*.pid; do [ -f \"$pid\" ] && kill -0 $(cat \"$pid\") 2>/dev/null && echo running && exit 0; done; echo stopped",
        vm::VM_RUN_PATH
    ))?;
    Ok(output.trim() == "running")
}

pub fn get_node_specs(node: &Node) -> NodeSpecs {
    match SshConnection::connect(&node.ip) {
        Ok(ssh) => {
            let mut specs = NodeSpecs::default();

            // Get CPU model
            if let Ok(output) = ssh.execute("cat /proc/cpuinfo | grep 'model name' | head -1 | cut -d':' -f2") {
                let cpu = output.trim();
                if !cpu.is_empty() {
                    specs.cpu = cpu.to_string();
                }
            }

            // Get CPU cores
            if let Ok(output) = ssh.execute("nproc 2>/dev/null || grep -c processor /proc/cpuinfo") {
                let cores = output.trim();
                if !cores.is_empty() {
                    specs.cores = format!("{} cores", cores);
                }
            }

            // Get RAM
            if let Ok(output) = ssh.execute("free -h | grep Mem | awk '{print $2}'") {
                let ram = output.trim();
                if !ram.is_empty() {
                    specs.ram = ram.to_string();
                }
            }

            // Get GPU
            if let Ok(output) = ssh.execute("lspci 2>/dev/null | grep -i 'vga\\|3d\\|display' | head -1 | cut -d':' -f3") {
                let gpu = output.trim();
                if !gpu.is_empty() {
                    specs.gpu = gpu.to_string();
                } else {
                    specs.gpu = "None".to_string();
                }
            }

            // Get disk info
            specs.disks = get_disk_info(&ssh);

            specs
        }
        Err(_) => NodeSpecs::default(),
    }
}

pub fn get_vm_status(node: &Node) -> Option<VmStatus> {
    match SshConnection::connect(&node.ip) {
        Ok(ssh) => {
            match vm::get_vm_info(&ssh, &node.hostname) {
                Ok(Some(info)) => Some(VmStatus {
                    running: true,
                    pid: Some(info.pid),
                }),
                Ok(None) => Some(VmStatus {
                    running: false,
                    pid: None,
                }),
                Err(_) => None,
            }
        }
        Err(_) => None,
    }
}

pub fn get_node_info(node: &Node) -> NodeInfo {
    let status = get_node_status(node);
    let specs = if status != NodeStatus::Offline {
        get_node_specs(node)
    } else {
        NodeSpecs::default()
    };
    let vm_info = if status != NodeStatus::Offline {
        get_vm_status(node)
    } else {
        None
    };

    NodeInfo {
        hostname: node.hostname.clone(),
        ip: node.ip.clone(),
        mac: node.mac.clone(),
        status,
        specs,
        vm_info,
    }
}
