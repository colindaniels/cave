use crate::config::Node;
use crate::ssh::SshConnection;

#[derive(Debug, Clone, PartialEq)]
pub enum NodeStatus {
    Offline,
    Standby,
    Active,
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
}

impl Default for NodeSpecs {
    fn default() -> Self {
        Self {
            cpu: "N/A".to_string(),
            cores: "N/A".to_string(),
            ram: "N/A".to_string(),
            gpu: "N/A".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub hostname: String,
    pub ip: String,
    pub mac: String,
    pub status: NodeStatus,
    pub specs: NodeSpecs,
}

pub fn get_node_status(node: &Node) -> NodeStatus {
    match SshConnection::connect(&node.ip) {
        Ok(ssh) => {
            // Check if there's an OS installed on the primary drive
            // In standby mode (live Alpine), there's no partition table on nvme
            let (output, status) = ssh
                .execute_with_status("lsblk -d -n -o NAME,TYPE | grep -E 'nvme|sda' | head -1")
                .unwrap_or_default();

            if status != 0 || output.trim().is_empty() {
                // No block device found, consider it standby
                return NodeStatus::Standby;
            }

            // Check if there's a filesystem on the drive
            let drive = output.split_whitespace().next().unwrap_or("nvme0n1");
            let (fs_output, fs_status) = ssh
                .execute_with_status(&format!("lsblk -f /dev/{} 2>/dev/null | grep -v '^NAME' | grep -v '^$' | head -1", drive))
                .unwrap_or_default();

            if fs_status != 0 || fs_output.trim().is_empty() || !fs_output.contains(|c: char| c.is_alphabetic()) {
                // No filesystem, this is standby mode
                NodeStatus::Standby
            } else {
                // Has filesystem, actively deployed
                NodeStatus::Active
            }
        }
        Err(_) => NodeStatus::Offline,
    }
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

            specs
        }
        Err(_) => NodeSpecs::default(),
    }
}

pub fn get_node_info(node: &Node) -> NodeInfo {
    let status = get_node_status(node);
    let specs = if status != NodeStatus::Offline {
        get_node_specs(node)
    } else {
        NodeSpecs::default()
    };

    NodeInfo {
        hostname: node.hostname.clone(),
        ip: node.ip.clone(),
        mac: node.mac.clone(),
        status,
        specs,
    }
}
