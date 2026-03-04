use crate::config::Node;
use crate::ssh::SshConnection;
use crate::vm;

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
