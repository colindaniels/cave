use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
    pub ip: String,
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

    pub fn add_node(&mut self, hostname: &str, ip: &str, mac: &str) -> Result<()> {
        if self.nodes.iter().any(|n| n.hostname == hostname) {
            anyhow::bail!("Node '{}' already exists", hostname);
        }

        self.nodes.push(Node {
            hostname: hostname.to_string(),
            ip: ip.to_string(),
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
