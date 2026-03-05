use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Read current ARP cache without network scan
fn read_arp_cache() -> HashMap<String, String> {
    let mut results = HashMap::new();
    if let Ok(output) = Command::new("ip").args(["neigh"]).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let ip = parts[0];
                    if let Some(pos) = parts.iter().position(|&x| x == "lladdr") {
                        if pos + 1 < parts.len() {
                            let mac = parts[pos + 1].to_lowercase();
                            if mac.contains(':') {
                                results.insert(mac, ip.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    results
}

/// Scan network once and return map of MAC -> IP
/// Pings cached IPs to refresh ARP entries with correct MACs
pub fn scan_network() -> HashMap<String, String> {
    // First, check existing ARP cache
    let cached = read_arp_cache();
    if !cached.is_empty() {
        // Ping cached IPs in parallel to refresh ARP
        let ips: Vec<_> = cached.values().take(20).cloned().collect();
        let handles: Vec<_> = ips
            .into_iter()
            .map(|ip| {
                std::thread::spawn(move || {
                    let _ = Command::new("ping")
                        .args(["-c", "1", "-W", "1", &ip])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                })
            })
            .collect();
        for h in handles {
            let _ = h.join();
        }
        // Re-read cache with fresh MAC->IP from ping responses
        return read_arp_cache();
    }

    // Cache empty - try arp-scan if available
    if let Ok(output) = Command::new("arp-scan")
        .args(["-l", "-q"])
        .output()
    {
        if output.status.success() {
            let mut results = HashMap::new();
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let ip = parts[0];
                    let mac = parts[1].to_lowercase();
                    if ip.contains('.') && mac.contains(':') {
                        results.insert(mac, ip.to_string());
                    }
                }
            }
            if !results.is_empty() {
                return results;
            }
        }
    }

    // Ping broadcast to populate cache
    let _ = Command::new("ping")
        .args(["-c", "1", "-W", "1", "-b", "192.168.1.255"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    read_arp_cache()
}

/// Scan network to find IP for a given MAC address
pub fn scan_for_ip(mac: &str) -> Option<String> {
    // Try cache first
    let cache = load_ip_cache();
    if let Some(ip) = cache.get(&mac.to_lowercase()) {
        return Some(ip.clone());
    }
    // Fall back to scan
    let results = scan_network();
    results.get(&mac.to_lowercase()).cloned()
}

/// Load IP cache from file (MAC -> IP mapping)
pub fn load_ip_cache() -> HashMap<String, String> {
    let cache_path = Config::cave_dir().join("ip_cache.json");
    if let Ok(content) = fs::read_to_string(&cache_path) {
        if let Ok(cache) = serde_json::from_str(&content) {
            return cache;
        }
    }
    HashMap::new()
}

/// Save IP cache to file
pub fn save_ip_cache(cache: &HashMap<String, String>) -> Result<()> {
    let cache_path = Config::cave_dir().join("ip_cache.json");
    let content = serde_json::to_string_pretty(cache)?;
    fs::write(&cache_path, content)?;
    Ok(())
}

/// Cached node information for instant cave list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedNode {
    pub hostname: String,
    pub mac: String,
    pub ip: Option<String>,
    pub status: String,  // "active", "standby", "offline"
    pub cpu: String,
    pub cores: String,
    pub ram: String,
    pub disks: Vec<CachedDisk>,
    pub vm: Option<CachedVm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedDisk {
    pub name: String,       // e.g., "nvme0n1", "sda"
    pub size_bytes: u64,
    pub disk_type: String,
    pub model: String,      // Disk model name
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedVm {
    pub name: String,
    pub ip: String,
    pub memory: String,
    pub cpus: String,
}

/// Load full node cache
pub fn load_node_cache() -> Vec<CachedNode> {
    let cache_path = Config::cave_dir().join("node_cache.json");
    if let Ok(content) = fs::read_to_string(&cache_path) {
        if let Ok(cache) = serde_json::from_str(&content) {
            return cache;
        }
    }
    Vec::new()
}

/// Save full node cache
pub fn save_node_cache(cache: &[CachedNode]) -> Result<()> {
    let cache_path = Config::cave_dir().join("node_cache.json");
    let content = serde_json::to_string_pretty(cache)?;
    fs::write(&cache_path, content)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub alpine_version: String,
    #[serde(default)]
    pub initialized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub hostname: String,
    pub mac: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                port: 8080,
                alpine_version: "3.21".to_string(),
                initialized: false,
            },
            nodes: Vec::new(),
        }
    }
}

impl Config {
    pub fn cave_dir() -> PathBuf {
        // If running with sudo, use the original user's home directory
        if let Ok(sudo_user) = std::env::var("SUDO_USER") {
            return PathBuf::from(format!("/home/{}/cave", sudo_user));
        }
        dirs::home_dir()
            .expect("Could not find home directory")
            .join("cave")
    }

    pub fn config_path() -> PathBuf {
        Self::cave_dir().join("config.toml")
    }

    pub fn images_dir() -> PathBuf {
        Self::cave_dir().join("images")
    }

    pub fn alpine_dir() -> PathBuf {
        Self::cave_dir().join("alpine")
    }

    pub fn ssh_dir() -> PathBuf {
        Self::cave_dir().join("ssh")
    }

    pub fn ssh_private_key() -> PathBuf {
        Self::ssh_dir().join("cave")
    }

    pub fn ssh_public_key() -> PathBuf {
        Self::ssh_dir().join("cave.pub")
    }

    pub fn pixiecore_pid_file() -> PathBuf {
        Self::cave_dir().join("pixiecore.pid")
    }

    pub fn server_log_file() -> PathBuf {
        Self::cave_dir().join("server.log")
    }

    pub fn vms_dir() -> PathBuf {
        Self::cave_dir().join("vms")
    }

    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config file: {:?}", config_path))?;
            toml::from_str(&content)
                .with_context(|| format!("Failed to parse config file: {:?}", config_path))
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();
        let cave_dir = Self::cave_dir();

        if !cave_dir.exists() {
            fs::create_dir_all(&cave_dir)
                .with_context(|| format!("Failed to create cave directory: {:?}", cave_dir))?;
        }

        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        fs::write(&config_path, content)
            .with_context(|| format!("Failed to write config file: {:?}", config_path))?;
        Ok(())
    }

    pub fn ensure_dirs() -> Result<()> {
        let dirs = [
            Self::cave_dir(),
            Self::images_dir(),
            Self::alpine_dir(),
            Self::ssh_dir(),
        ];

        for dir in dirs {
            if !dir.exists() {
                fs::create_dir_all(&dir)
                    .with_context(|| format!("Failed to create directory: {:?}", dir))?;
            }
        }
        Ok(())
    }

    pub fn add_node(&mut self, hostname: &str, mac: &str) -> Result<()> {
        if self.nodes.iter().any(|n| n.hostname == hostname) {
            anyhow::bail!("Node '{}' already exists", hostname);
        }

        self.nodes.push(Node {
            hostname: hostname.to_string(),
            mac: mac.to_string(),
        });
        Ok(())
    }

    pub fn remove_node(&mut self, hostname: &str) -> Result<()> {
        let initial_len = self.nodes.len();
        self.nodes.retain(|n| n.hostname != hostname);

        if self.nodes.len() == initial_len {
            anyhow::bail!("Node '{}' not found", hostname);
        }
        Ok(())
    }

    pub fn get_node(&self, hostname: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.hostname == hostname)
    }
}
